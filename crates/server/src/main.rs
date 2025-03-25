mod handlers;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{
    routing::{get, post},
    Json, Router,
};
use handlers::runner::runner_handler;
use reqwest::Client;
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/v1/run", post(runner_handler))
        .route("/v2/run", post(proxy_runner_handler))
        .route("/v2/prove", post(proxy_prover_handler))
        .route("/_ah/warmup", get(|| async { "OK" }))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

    println!("Listening on 0.0.0.0:3000");

    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}

async fn health_check() -> StatusCode {
    StatusCode::OK
}

async fn proxy_runner_handler(Json(payload): Json<serde_json::Value>) -> Response {
    let client = Client::new();
    let res = client
        .post("http://51.15.233.88:3000/v1/run")
        .json(&payload)
        .send()
        .await;

    match res {
        Ok(response) => {
            let status: StatusCode = match response.status() {
                reqwest::StatusCode::OK => StatusCode::OK,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read response".into());
            (status, body).into_response()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Failed to proxy request").into_response(),
    }
}

async fn proxy_prover_handler(Json(payload): Json<serde_json::Value>) -> Response {
    let client = Client::new();
    let res = client
        .post("http://51.15.233.88:3000/v1/prove")
        .json(&payload)
        .send()
        .await;

    match res {
        Ok(response) => {
            let status: StatusCode = match response.status() {
                reqwest::StatusCode::OK => StatusCode::OK,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read response".into());
            (status, body).into_response()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Failed to proxy request").into_response(),
    }
}