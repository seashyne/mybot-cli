use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::screen::draw;
use super::{Action, State, Step};
use crate::providers::presets;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn select(name: &str) -> State {
    let mut state = State::new();
    state.choice = presets::PRESETS
        .iter()
        .position(|preset| preset.name == name)
        .unwrap();
    state
}

#[test]
fn cloud_provider_walks_provider_key_model() {
    let mut state = State::new();
    assert!(matches!(
        state.handle_key(key(KeyCode::Enter)),
        Action::Continue
    ));
    assert_eq!(state.step, Step::Key);

    for ch in "sk-ant-123456".chars() {
        state.handle_key(key(KeyCode::Char(ch)));
    }
    assert!(matches!(
        state.handle_key(key(KeyCode::Enter)),
        Action::Continue
    ));
    assert_eq!(state.step, Step::Model);

    match state.handle_key(key(KeyCode::Enter)) {
        Action::Done(config, name) => {
            assert_eq!(name, "anthropic");
            assert_eq!(config.model.as_deref(), Some("claude-sonnet-4-5"));
            assert_eq!(config.api_key.as_deref(), Some("sk-ant-123456"));
            assert!(config.base_url.is_none());
        }
        _ => panic!("expected the wizard to finish"),
    }
}

#[test]
fn local_provider_skips_the_key_step() {
    let mut state = select("lmstudio");
    state.handle_key(key(KeyCode::Enter));
    assert_eq!(state.step, Step::Model);
    assert_eq!(state.base_url, "http://127.0.0.1:1234/v1");

    match state.handle_key(key(KeyCode::Enter)) {
        Action::Done(config, name) => {
            assert_eq!(name, "lmstudio");
            assert_eq!(config.model.as_deref(), Some("auto"));
            assert!(config.api_key.is_none());
        }
        _ => panic!("expected the wizard to finish"),
    }
}

#[test]
fn third_party_presets_keep_their_endpoint() {
    let mut state = select("deepseek");
    state.handle_key(key(KeyCode::Enter));
    for ch in "sk-deepseek-key".chars() {
        state.handle_key(key(KeyCode::Char(ch)));
    }
    state.handle_key(key(KeyCode::Enter));
    state.handle_key(key(KeyCode::Enter));

    match state.handle_key(key(KeyCode::Enter)) {
        Action::Done(config, name) => {
            assert_eq!(name, "deepseek");
            assert_eq!(
                config.base_url.as_deref(),
                Some("https://api.deepseek.com/v1")
            );
            assert_eq!(config.api_key_env.as_deref(), Some("DEEPSEEK_API_KEY"));
            assert_eq!(config.model.as_deref(), Some("deepseek-chat"));
        }
        _ => panic!("expected the wizard to finish"),
    }
}

#[test]
fn custom_provider_requires_a_base_url() {
    let mut state = select("custom");
    state.handle_key(key(KeyCode::Enter));
    assert_eq!(state.step, Step::BaseUrl);

    assert!(matches!(
        state.handle_key(key(KeyCode::Enter)),
        Action::Continue
    ));
    assert!(state.error.is_some());
    assert_eq!(state.step, Step::BaseUrl);

    for ch in "http://10.0.0.5:8080/v1".chars() {
        state.handle_key(key(KeyCode::Char(ch)));
    }
    state.handle_key(key(KeyCode::Enter));
    assert_eq!(state.step, Step::Key);

    // an empty key is fine for custom endpoints
    state.handle_key(key(KeyCode::Enter));
    assert_eq!(state.step, Step::Model);
}

#[test]
fn empty_key_is_rejected_for_cloud_providers() {
    let mut state = State::new();
    state.handle_key(key(KeyCode::Enter));
    state.handle_key(key(KeyCode::Enter));
    assert_eq!(state.step, Step::Key);
    assert!(state.error.is_some());
}

#[test]
fn esc_from_the_first_step_cancels() {
    let mut state = State::new();
    assert!(matches!(
        state.handle_key(key(KeyCode::Esc)),
        Action::Cancel
    ));
}

#[test]
fn backspace_edits_the_current_field() {
    let mut state = State::new();
    state.handle_key(key(KeyCode::Enter));
    for ch in "abc".chars() {
        state.handle_key(key(KeyCode::Char(ch)));
    }
    state.handle_key(key(KeyCode::Backspace));
    assert_eq!(state.key, "ab");
}

#[test]
fn typing_on_the_provider_step_does_not_touch_the_key() {
    let mut state = State::new();
    state.handle_key(key(KeyCode::Char('x')));
    assert!(state.key.is_empty());
}

fn screen(state: &mut State, width: u16, height: u16) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| draw(frame, state)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn draws_the_provider_list() {
    let mut state = State::new();
    let text = screen(&mut state, 90, 30);
    assert!(text.contains("mybot setup"));
    assert!(text.contains("Choose a provider"));
    assert!(text.contains("Anthropic"));
    assert!(text.contains("DeepSeek"));
}

#[test]
fn draws_the_key_step_masked() {
    let mut state = State::new();
    state.handle_key(key(KeyCode::Enter));
    for ch in "sk-secret-value".chars() {
        state.handle_key(key(KeyCode::Char(ch)));
    }
    let text = screen(&mut state, 90, 30);
    assert!(text.contains("api key"));
    assert!(text.contains("••••"));
    assert!(!text.contains("sk-secret-value"));
}

#[test]
fn list_scrolls_to_keep_the_selection_visible() {
    let mut state = State::new();
    for _ in 0..presets::PRESETS.len() - 1 {
        state.handle_key(key(KeyCode::Down));
    }
    let last = state.choice().label;
    let text = screen(&mut state, 90, 24);
    assert!(
        text.contains(last),
        "the selected provider should be on screen"
    );
}
