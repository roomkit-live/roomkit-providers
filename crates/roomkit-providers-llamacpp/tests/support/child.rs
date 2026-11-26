//! A stand-in llama-server run as a real child on loopback.

use axum::{
    Json, Router,
    extract::State,
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{env, error::Error, fs::OpenOptions, io::Write, path::PathBuf};

pub(crate) fn argument(name: &str) -> Option<String> {
    env::args().skip_while(|a| a != name).nth(1)
}

pub(crate) async fn serve(port: u16) -> Result<(), Box<dyn Error>> {
    let record = argument("--record").map(PathBuf::from);
    if let Some(path) = &record {
        writeln!(
            OpenOptions::new().create(true).append(true).open(path)?,
            "{}",
            json!(env::args().skip(1).collect::<Vec<_>>())
        )?;
    }
    if env::args().any(|a| a == "--fail-load") {
        return Err("cannot load model".into());
    }
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    if let Some(signal) = argument("--wait-for-cancellation") {
        std::fs::write(signal, "bound")?;
        std::future::pending::<()>().await;
    }
    let app = Router::new()
        .route("/health", get(|| async { Json(json!({"status":"ok"})) }))
        .route(
            "/v1/models",
            get(|| async { Json(json!({"data":[{"id":"local-model"}]})) }),
        )
        .route("/v1/chat/completions", post(answer))
        .with_state(record);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn answer(
    State(record): State<Option<PathBuf>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if let Some(file) = record.and_then(|p| OpenOptions::new().append(true).open(p).ok()) {
        let _written = writeln!(&file, "{body}");
    }
    let call =
        json!({"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{}"}});
    if body.get("stream") == Some(&Value::Bool(true)) {
        let mut delta = call;
        if let Some(fields) = delta.as_object_mut() {
            fields.insert("index".into(), 0.into());
        }
        let chunk = json!({"choices":[{"index":0,"delta":{"tool_calls":[delta]},"finish_reason":"tool_calls"}]});
        return (
            [("content-type", "text/event-stream")],
            format!("data: {chunk}\n\ndata: [DONE]\n\n"),
        )
            .into_response();
    }
    Json(json!({"choices":[{"index":0,"message":{"role":"assistant","content":"done","tool_calls":[call]},"finish_reason":"tool_calls"}]})).into_response()
}
