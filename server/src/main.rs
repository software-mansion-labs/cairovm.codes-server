mod compilation;
mod errors;
mod executables_runner;
mod prover;
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
use prover::prove_and_verify;
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tracer::trace::TracerData;
use utils::{
    SierraFormattedProgram, create_named_folder, create_temp_folder, process_args,
    write_binary_to_file, write_string_to_file,
};

pub const CAIRO_LANG_COMPILER_VERSION: &str = "2.10.1";

#[derive(Serialize, Deserialize)]
pub struct RunnerPayload {
    pub cairo_program_code: String,
    pub program_arguments: Option<String>,
    pub proof_required: Option<bool>,
    pub verification_required: Option<bool>,
}

#[derive(Serialize, Deserialize)]
pub struct ProverPayload {
    pub air_public_input: String,
    pub air_private_input: String,
    pub trace: Vec<u8>,
    pub memory: Vec<u8>,
    pub folder_path: String,
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
    compilation_time_ms: u64,
    execution_time_ms: u64,
    proving_is_not_supported: bool,
    air_public_input: Option<String>,
    air_private_input: Option<String>,
    trace: Option<Vec<u8>>,
    memory: Option<Vec<u8>>,
    folder_path: String,
}

#[derive(Serialize)]
pub struct ProverResult {
    proof: String,
    proving_time_ms: u64,
    verification_time_ms: u64,
}

pub async fn runner_handler(
    Json(payload): Json<RunnerPayload>,
) -> Result<Json<RunnerResult>, ResponseError> {
    let (project_path, folder_name) = create_temp_folder();
    let file_path = write_string_to_file(&project_path, "main.cairo", &payload.cairo_program_code);
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
        folder_name,
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

pub async fn prover_handler(
    Json(payload): Json<ProverPayload>,
) -> Result<Json<ProverResult>, ResponseError> {
    let project_path = create_named_folder(&payload.folder_path);

    write_string_to_file(
        &project_path,
        "air_public_input.txt",
        &payload.air_public_input,
    );

    write_string_to_file(
        &project_path,
        "air_private_input.txt",
        &payload.air_private_input,
    );

    write_binary_to_file(&project_path, "trace.bin", &payload.trace);

    write_binary_to_file(&project_path, "memory.bin", &payload.memory);

    let (proof, proving_time_ms, verification_time_ms) =
        prove_and_verify(&project_path).map_err(|err| {
            fs::remove_dir_all(&project_path).expect("Failed to delete temporary folder");
            ResponseError::get_error(Error::Anyhow(err))
        })?;

    fs::remove_dir_all(&project_path).expect("Failed to delete temporary folder");

    Ok(Json(ProverResult {
        proof,
        proving_time_ms,
        verification_time_ms,
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/v1/run", post(runner_handler))
        .route("/v1/prove", post(prover_handler))
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
