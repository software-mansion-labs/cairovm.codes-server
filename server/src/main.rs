mod compilation;
mod errors;
mod executables_runner;
mod runner;
mod runner_utils;
mod tracer;
mod utils;

use std::collections::HashMap;
use std::fs;

use axum::http::StatusCode;
use axum::{
    Json, Router,
    routing::{get, post},
};
use cairo_lang_runner::Arg;
use errors::{Error, LogEntry, ResponseError};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tracer::trace::TracerData;
use utils::{SierraFormattedProgram, process_args, write_to_temp_file};

pub const CAIRO_LANG_COMPILER_VERSION: &str = "2.10.1";

#[derive(Serialize, Deserialize)]
pub struct RunnerPayload {
    pub cairo_program_code: String,
    pub program_arguments: Option<String>,
    pub proof_required: Option<bool>,
    pub verification_required: Option<bool>,
}

#[derive(Serialize)]
pub struct RunnerResult {
    cairo_lang_compiler_version: String,
    serialized_output: Option<String>,
    execution_panic_message: Option<String>,
    is_compilation_successful: bool,
    is_execution_successful: bool,
    tracer_data: TracerData,
    casm_formatted_instructions: Vec<String>,
    casm_to_sierra_map: HashMap<usize, Vec<usize>>,
    sierra_formatted_program: SierraFormattedProgram,
    logs: Vec<LogEntry>,
    proof: Option<String>,
    compilation_time_ms: u64,
    execution_time_ms: u64,
    proving_time_ms: Option<u64>,
    verification_time_ms: Option<u64>,
    proving_is_not_supported: bool,
}

pub async fn runner_handler(
    Json(payload): Json<RunnerPayload>,
) -> Result<Json<RunnerResult>, ResponseError> {
    let (file_path, project_path) = write_to_temp_file(&payload.cairo_program_code);
    let user_args: Vec<Arg> = match payload.program_arguments {
        Some(args) => process_args(&args)
            .map_err(|err| ResponseError::get_error(Error::Anyhow(anyhow::anyhow!(err))))?,
        None => vec![],
    };

    let is_contract = payload.cairo_program_code.contains("#[starknet::contract]");
    let is_executable = payload.cairo_program_code.contains("#[executable]");

    if is_contract && is_executable {
        return Err(ResponseError::get_error(Error::Anyhow(anyhow::anyhow!(
            "Cannot be both contract and executable"
        ))));
    }

    let run = if is_contract {
        runner::run
    } else {
        executables_runner::run
    };

    let runner_path = if is_contract {
        file_path.clone()
    } else {
        project_path.clone()
    };

    let runner_result = match run(
        runner_path,
        user_args,
        payload.proof_required.unwrap_or(false),
        payload.verification_required.unwrap_or(false),
    ) {
        Ok(result) => result,
        Err(e) => {
            // println!("Error: {:?}", e);
            fs::remove_dir_all(&project_path).expect("Failed to delete temporary folder");
            return Err(e);
        }
    };
    fs::remove_dir_all(&project_path).expect("Failed to delete temporary folder");
    Ok(Json(runner_result))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/v1/run", post(runner_handler))
        .route("/_ah/warmup", get(|| async { "OK" }))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

    println!("Listening on 0.0.0.0:3000");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}

async fn health_check() -> StatusCode {
    StatusCode::OK
}
