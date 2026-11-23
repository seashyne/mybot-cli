mod app;
mod commands;
mod composer;
mod highlight;
mod markdown;
mod permission;
mod render;
mod setup;
mod term;
mod theme;
mod todo;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use crossterm::event::Event;
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::agent::{Agent, AgentEvent, AgentOptions};
use crate::config::{Config, PermissionMode};
use crate::error::{AppError, Result};
use crate::permissions::{Decision, PermissionEngine, PermissionRequest, Prompter};
use crate::prompt::{self, PromptOptions};
use crate::providers;
use crate::session::Session;
use crate::tools::ToolRegistry;

use app::{App, Ctx, Status, UiEvent};

pub struct TuiOptions {
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub permission: Option<PermissionMode>,
    pub system_override: Option<String>,
    pub append_system: Vec<String>,
    pub session_id: Option<String>,
    pub continue_: bool,
    pub initial_prompt: Option<String>,
}

struct PermissionAsk {
    request: PermissionRequest,
    reply: oneshot::Sender<Decision>,
}

struct ChannelPrompter {
    tx: mpsc::UnboundedSender<PermissionAsk>,
}

#[async_trait]
impl Prompter for ChannelPrompter {
    async fn ask(&self, request: &PermissionRequest) -> Decision {
        let (reply, response) = oneshot::channel();
        if self
            .tx
            .send(PermissionAsk {
                request: request.clone(),
                reply,
            })
            .is_err()
        {
            return Decision::Deny;
        }
        response.await.unwrap_or(Decision::Deny)
    }
}

pub async fn run(options: TuiOptions) -> Result<()> {
    let cwd = options
        .cwd
        .canonicalize()
        .unwrap_or_else(|_| options.cwd.clone());

    let mut config = Config::load()?;
    let permission = options.permission.unwrap_or(config.permissions.mode);

    term::install_panic_hook();
    let _guard = term::TerminalGuard::enter()
        .map_err(|error| AppError::runtime(format!("cannot initialise terminal: {error}")))?;
    let mut terminal = term::terminal()
        .map_err(|error| AppError::runtime(format!("cannot create terminal: {error}")))?;

    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<Event>();
    std::thread::spawn(move || {
        while let Ok(event) = crossterm::event::read() {
            if input_tx.send(event).is_err() {
                break;
            }
        }
    });

    if !config.is_configured() && !setup::run(&mut terminal, &mut input_rx, &mut config).await? {
        return Ok(());
    }

    let resolved = config.resolve(options.model.as_deref())?;
    let provider: Arc<dyn crate::providers::Provider> =
        Arc::from(providers::build(&resolved)?);

    let skills = Arc::new(crate::skills::SkillSet::discover(&cwd));
    let plugins = Arc::new(crate::plugins::PluginSet::discover(&cwd));
    let memory = crate::memory::MemoryStore::load(&cwd);
    let mcp = Arc::new(crate::mcp::McpManager::start(&cwd).await);
    let agents = Arc::new(crate::subagents::AgentSet::discover(&cwd));
    let system = prompt::build(&PromptOptions {
        cwd: cwd.clone(),
        system_override: options.system_override.clone(),
        append: options.append_system.clone(),
        skills: skills.prompt_block(),
        memory: memory.prompt_block(),
        agents: agents.prompt_block(),
    });
    let mut registry = ToolRegistry::with_skills(skills.clone());
    for tool in mcp.tools() {
        registry.register(tool);
    }

    let session = if let Some(id) = &options.session_id {
        Session::open(id)?
    } else if options.continue_ {
        match Session::latest_for_cwd(&cwd)? {
            Some(id) => Session::open(&id)?,
            None => Session::create(&cwd, &resolved.name, &resolved.model)?,
        }
    } else {
        Session::create(&cwd, &resolved.name, &resolved.model)?
    };

    let session_id = app::short_id(session.id());
    let messages = session.messages.clone();
    let session = Arc::new(Mutex::new(session));

    let (perm_tx, mut perm_rx) = mpsc::unbounded_channel::<PermissionAsk>();
    let prompter = Arc::new(ChannelPrompter { tx: perm_tx });
    let permissions = Arc::new(PermissionEngine::with_sandbox(
        permission,
        config.permissions.sandbox,
        cwd.clone(),
        config.permissions.rules.clone(),
    ));
    let retry = providers::RetryPolicy::from_resolved(&resolved);

    let runtime = Arc::new(crate::subagents::AgentRuntime {
        provider: provider.clone(),
        permissions: permissions.clone(),
        prompter: prompter.clone(),
        base_system: system.clone(),
        cwd: cwd.clone(),
        max_tokens: resolved.max_tokens,
        temperature: resolved.temperature,
        thinking_budget: resolved.thinking_budget,
        retry,
        skills: skills.clone(),
        mcp: Some(mcp.clone()),
        agents: agents.clone(),
    });

    let (tx, mut rx) = mpsc::channel::<AgentEvent>(512);
    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel::<UiEvent>();
    let team = Arc::new(crate::team::Team::new(runtime.clone(), tx.clone()));
    registry.register(Arc::new(crate::subagents::AgentTool::new(runtime)));
    crate::team::register_lead(&mut registry, team.clone());
    let tools = Arc::new(registry);

    let agent_value = Agent::new(AgentOptions {
        provider,
        tools: tools.clone(),
        permissions: permissions.clone(),
        prompter,
        system,
        cwd: cwd.clone(),
        max_tokens: resolved.max_tokens,
        temperature: resolved.temperature,
        thinking_budget: resolved.thinking_budget,
        max_steps: None,
        session: Some(session),
        messages,
        retry,
    });
    let checkpoints = agent_value.checkpoints();
    let agent = Arc::new(Mutex::new(agent_value));

    let status = Status {
        provider: resolved.name.clone(),
        model: resolved.model.clone(),
        permission,
        thinking_budget: resolved.thinking_budget,
        session: session_id,
        input_tokens: 0,
        output_tokens: 0,
        total_input_tokens: 0,
        total_output_tokens: 0,
    };

    let mut app = App::new(status, cwd, options.initial_prompt);
    for warning in mcp.warnings() {
        app.transcript
            .push(app::Entry::Info(format!("! MCP: {warning}")));
    }
    let ctx = Ctx {
        agent: agent.clone(),
        tx: tx.clone(),
        ui_tx,
        skills,
        plugins,
        mcp: mcp.clone(),
        agents,
        permissions: permissions.clone(),
        tools,
        checkpoints,
    };

    app.start_initial(&ctx);

    if resolved.kind == crate::config::ProviderKind::OpenAi
        && resolved.model.eq_ignore_ascii_case("auto")
    {
        let probe_resolved = resolved.clone();
        let probe_tx = tx.clone();
        tokio::spawn(async move {
            if let Ok(mut models) = crate::providers::openai::list_models(&probe_resolved).await
                && let Some(first) = models.drain(..).next()
            {
                let _ = probe_tx.send(AgentEvent::ModelName(first)).await;
            }
        });
    }

    let mut ticker = tokio::time::interval(Duration::from_millis(100));
    loop {
        terminal
            .draw(|frame| render::draw(frame, &mut app))
            .map_err(|error| AppError::runtime(format!("cannot draw: {error}")))?;

        tokio::select! {
            Some(event) = input_rx.recv() => match event {
                Event::Key(key) => app.handle_key(key, &ctx),
                Event::Paste(text) => app.handle_paste(&text, &ctx),
                _ => {}
            },
            Some(agent_event) = rx.recv() => app.handle_agent(agent_event),
            Some(ui_event) = ui_rx.recv() => app.handle_ui(ui_event),
            Some(ask) = perm_rx.recv() => app.set_permission(ask.request, ask.reply),
            _ = ticker.tick() => app.tick(),
        }

        if app.quit {
            break;
        }
    }

    // Let a running turn kill its child processes before the process exits.
    team.cancel();
    if app.busy {
        app.cancel.cancel();
        tokio::time::sleep(Duration::from_millis(400)).await;
    }

    Ok(())
}
