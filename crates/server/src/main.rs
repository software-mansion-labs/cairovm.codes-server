mod handlers;
use axum::{routing::{get, post}, Json, Router};
use axum::http::StatusCode;
use handlers::runner::runner_handler;
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/v1/run", post(runner_handler))
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
