use std::process::Command;
use std::sync::{Arc, Mutex};

use assert_cmd::prelude::*;
use axum::Router;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use predicates::prelude::*;

const SSE_BODY: &str = concat!(
    "data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello from local model\"},\"finish_reason\":null}]}\n\n",
    "data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":4}}\n\n",
    "data: [DONE]\n\n",
);

const MODELS_BODY: &str =
    r#"{"object":"list","data":[{"id":"local-model","object":"model","owned_by":"local"}]}"#;

async fn models_handler() -> impl IntoResponse {
    (
        [("content-type", "application/json")],
        MODELS_BODY.to_string(),
    )
}

async fn chat_handler(
    axum::extract::State(bodies): axum::extract::State<Arc<Mutex<Vec<String>>>>,
    body: String,
) -> impl IntoResponse {
    bodies.lock().unwrap().push(body);
    (
        [("content-type", "text/event-stream")],
        SSE_BODY.to_string(),
    )
}

/// A minimal LM Studio / Ollama style server: `/v1/models` + streaming
/// `/v1/chat/completions`, no API key required.
async fn start_local_server() -> (String, Arc<Mutex<Vec<String>>>) {
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/models", get(models_handler))
        .route("/v1/chat/completions", post(chat_handler))
        .with_state(bodies.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{address}/v1"), bodies)
}

fn write_config(home: &std::path::Path, base_url: &str) {
    let config = format!(
        "default_provider = \"lmstudio\"\n\n[providers.lmstudio]\nkind = \"openai\"\nbase_url = \"{base_url}\"\nmodel = \"auto\"\nmax_tokens = 256\n"
    );
    std::fs::write(home.join("config.toml"), config).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn exec_works_with_local_openai_server_and_no_key() {
    let (base_url, bodies) = start_local_server().await;
    let home = tempfile::tempdir().unwrap();
    write_config(home.path(), &base_url);

    let mut command = Command::cargo_bin("mybot").unwrap();
    command
        .args(["exec", "hi"])
        .env("MYBOT_HOME", home.path())
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY");

    command
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello from local model"));

    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 1, "expected exactly one chat request");
    assert!(
        bodies[0].contains("\"model\":\"local-model\""),
        "auto model was not resolved from /v1/models: {}",
        bodies[0]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn auto_model_reports_missing_models_clearly() {
    // A server that answers /v1/models with an empty list.
    async fn empty_models() -> impl IntoResponse {
        (
            [("content-type", "application/json")],
            r#"{"object":"list","data":[]}"#.to_string(),
        )
    }

    let app = Router::new().route("/v1/models", get(empty_models));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let home = tempfile::tempdir().unwrap();
    write_config(home.path(), &format!("http://{address}/v1"));

    let mut command = Command::cargo_bin("mybot").unwrap();
    command
        .args(["exec", "hi"])
        .env("MYBOT_HOME", home.path())
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY");

    command
        .assert()
        .code(3)
        .stderr(predicate::str::contains("no chat model available"));
}
