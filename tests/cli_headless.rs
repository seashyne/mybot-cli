use std::process::Command;
use std::sync::{Arc, Mutex};

use assert_cmd::prelude::*;
use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use predicates::prelude::*;

const SSE_BODY: &str = concat!(
    "event: message_start\n",
    "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":0}}}\n\n",
    "event: content_block_start\n",
    "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello from fake provider\"}}\n\n",
    "event: content_block_stop\n",
    "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
    "event: message_delta\n",
    "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\n",
    "event: message_stop\n",
    "data: {\"type\":\"message_stop\"}\n\n",
);

async fn anthropic_handler() -> impl IntoResponse {
    (
        [("content-type", "text/event-stream")],
        SSE_BODY.to_string(),
    )
}

async fn capturing_handler(
    axum::extract::State(bodies): axum::extract::State<Arc<Mutex<Vec<String>>>>,
    body: String,
) -> impl IntoResponse {
    bodies.lock().unwrap().push(body);
    (
        [("content-type", "text/event-stream")],
        SSE_BODY.to_string(),
    )
}

/// Start a fake server that records every request body.
async fn start_capturing_server() -> (String, Arc<Mutex<Vec<String>>>) {
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/messages", post(capturing_handler))
        .with_state(bodies.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{address}"), bodies)
}

/// First request is rate limited, later ones succeed. Counts every request.
async fn flaky_handler(State(attempts): State<Arc<Mutex<u32>>>) -> Response {
    let mut attempts = attempts.lock().unwrap();
    *attempts += 1;
    if *attempts == 1 {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, "0")],
            r#"{"error":{"message":"slow down"}}"#,
        )
            .into_response();
    }
    (
        [(header::CONTENT_TYPE, "text/event-stream")],
        SSE_BODY,
    )
        .into_response()
}

/// Always rejects the request, counting attempts.
async fn bad_request_handler(State(attempts): State<Arc<Mutex<u32>>>) -> Response {
    *attempts.lock().unwrap() += 1;
    (
        StatusCode::BAD_REQUEST,
        r#"{"error":{"message":"bad model"}}"#,
    )
        .into_response()
}

async fn start_flaky_server() -> (String, Arc<Mutex<u32>>) {
    let attempts = Arc::new(Mutex::new(0u32));
    let app = Router::new()
        .route("/v1/messages", post(flaky_handler))
        .with_state(attempts.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{address}"), attempts)
}

async fn start_bad_request_server() -> (String, Arc<Mutex<u32>>) {
    let attempts = Arc::new(Mutex::new(0u32));
    let app = Router::new()
        .route("/v1/messages", post(bad_request_handler))
        .with_state(attempts.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{address}"), attempts)
}

/// Start a fake Anthropic-compatible server and return its base URL.
async fn start_server() -> String {
    let app = Router::new().route("/v1/messages", post(anthropic_handler));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{address}")
}

fn write_config(home: &std::path::Path, base_url: &str, with_key: bool) {
    let key_line = if with_key {
        "api_key = \"test-key\"\n"
    } else {
        ""
    };
    let config = format!(
        "default_provider = \"fake\"\n\n[providers.fake]\nkind = \"anthropic\"\nbase_url = \"{base_url}\"\n{key_line}model = \"test-model\"\nmax_tokens = 256\n"
    );
    std::fs::write(home.join("config.toml"), config).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn exec_prints_streamed_answer() {
    let base_url = start_server().await;
    let home = tempfile::tempdir().unwrap();
    write_config(home.path(), &base_url, true);

    let mut command = Command::cargo_bin("mybot").unwrap();
    command
        .args(["exec", "say hello"])
        .env("MYBOT_HOME", home.path())
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY");

    command
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello from fake provider"));
}

#[tokio::test(flavor = "multi_thread")]
async fn exec_json_format_has_result_shape() {
    let base_url = start_server().await;
    let home = tempfile::tempdir().unwrap();
    write_config(home.path(), &base_url, true);

    let mut command = Command::cargo_bin("mybot").unwrap();
    command
        .args(["exec", "hi", "--output-format", "json"])
        .env("MYBOT_HOME", home.path())
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY");

    command
        .assert()
        .success()
        .stdout(predicate::str::contains("\"type\": \"exec.result\""))
        .stdout(predicate::str::contains("Hello from fake provider"));
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_api_key_exits_3() {
    let home = tempfile::tempdir().unwrap();
    write_config(home.path(), "https://api.example.invalid", false);

    let mut command = Command::cargo_bin("mybot").unwrap();
    command
        .args(["exec", "hi"])
        .env("MYBOT_HOME", home.path())
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY");

    command.assert().code(3);
}

#[test]
fn missing_prompt_exits_2() {
    let home = tempfile::tempdir().unwrap();
    let mut command = Command::cargo_bin("mybot").unwrap();
    command
        .args(["exec"])
        .env("MYBOT_HOME", home.path())
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY");

    command.assert().code(2);
}

#[test]
fn help_lists_subcommands() {
    let mut command = Command::cargo_bin("mybot").unwrap();
    command
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("exec"))
        .stdout(predicate::str::contains("provider"));
}

#[test]
fn session_is_persisted_after_exec() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    // A closed port: the session is created before the request fails.
    write_config(home.path(), "http://127.0.0.1:9", true);

    let mut command = Command::cargo_bin("mybot").unwrap();
    command
        .args(["exec", "hello", "--timeout", "5"])
        .current_dir(cwd.path())
        .env("MYBOT_HOME", home.path())
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY");

    let _ = command.assert();

    let sessions = home.path().join("sessions");
    let count = std::fs::read_dir(&sessions)
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert!(count >= 1, "expected a session ledger in {sessions:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn retries_rate_limited_requests() {
    let (base_url, attempts) = start_flaky_server().await;
    let home = tempfile::tempdir().unwrap();
    write_config(home.path(), &base_url, true);

    let mut command = Command::cargo_bin("mybot").unwrap();
    command
        .args(["exec", "hi"])
        .env("MYBOT_HOME", home.path())
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY");

    command
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello from fake provider"))
        .stderr(predicate::str::contains("retry 1/2"));

    assert_eq!(*attempts.lock().unwrap(), 2, "expected one retry");
}

#[tokio::test(flavor = "multi_thread")]
async fn does_not_retry_client_errors() {
    let (base_url, attempts) = start_bad_request_server().await;
    let home = tempfile::tempdir().unwrap();
    write_config(home.path(), &base_url, true);

    let mut command = Command::cargo_bin("mybot").unwrap();
    command
        .args(["exec", "hi"])
        .env("MYBOT_HOME", home.path())
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY");

    command
        .assert()
        .code(3)
        .stderr(predicate::str::contains("HTTP 400"));

    assert_eq!(*attempts.lock().unwrap(), 1, "400 must not be retried");
}

#[tokio::test(flavor = "multi_thread")]
async fn continue_replays_previous_messages() {
    let (base_url, bodies) = start_capturing_server().await;
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    write_config(home.path(), &base_url, true);

    let run = |args: &[&str]| {
        let mut command = Command::cargo_bin("mybot").unwrap();
        command
            .args(args)
            .current_dir(cwd.path())
            .env("MYBOT_HOME", home.path())
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("OPENAI_API_KEY");
        command.assert().success();
    };

    run(&["exec", "first message"]);
    run(&["exec", "second message", "-c"]);

    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 2, "expected two model requests");
    assert!(bodies[1].contains("first message"), "history was not replayed");
    assert!(bodies[1].contains("second message"));
    assert!(
        bodies[1].contains("Hello from fake provider"),
        "assistant reply was not replayed"
    );
}
