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
use std::net::SocketAddr;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{ConnectInfo, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::any;
use axum::{Router, routing::get};
use axum_extra::{TypedHeader, headers};
use cairo_lang_runner::Arg;
use errors::{Error, LogEntry, ResponseError};
use futures_util::{SinkExt, StreamExt};
use prover::prove_and_verify;
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;
use tokio::time::timeout;
use tower_http::cors::CorsLayer;
use tracer::trace::TracerData;
use utils::{SierraFormattedProgram, create_temp_folder, process_args, write_to_file};

pub const CAIRO_LANG_COMPILER_VERSION: &str = "2.10.1";

#[derive(Serialize, Deserialize, Debug)]
pub struct RunnerPayload {
    pub cairo_program_code: String,
    pub program_arguments: Option<String>,
    pub proof_required: Option<bool>,
    pub verification_required: Option<bool>,
}

#[derive(Serialize, Debug)]
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
    proof_required: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/ws", any(ws_handler))
        .route("/health", get(health_check))
        .route("/_ah/warmup", get(|| async { "OK" }))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

    println!("Listening on 0.0.0.0:3000");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();

    Ok(())
}

async fn health_check() -> StatusCode {
    StatusCode::OK
}

#[derive(Serialize)]
pub struct ProverResult {
    proof: String,
    proving_time_ms: u64,
    verification_time_ms: u64,
}

#[derive(Serialize)]
pub enum ServerMessage {
    RunnerResult(RunnerResult),
    ProverResult(ProverResult),
    CompilerAndRunnerError(String),
    ProverAndVerifierError(String),
}

/// The handler for the HTTP request (this gets called when the HTTP request lands at the start
/// of websocket negotiation). After this completes, the actual switching from HTTP to
/// websocket protocol will occur.
/// This is the last point where we can extract TCP/IP metadata such as IP address of the client
/// as well as things from HTTP headers such as user-agent of the browser etc.
async fn ws_handler(
    ws: WebSocketUpgrade,
    user_agent: Option<TypedHeader<headers::UserAgent>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let user_agent = if let Some(TypedHeader(user_agent)) = user_agent {
        user_agent.to_string()
    } else {
        String::from("Unknown browser")
    };

    // finalize the upgrade process by returning upgrade callback.
    // we can customize the callback by sending additional info such as address.
    ws.on_upgrade(move |socket| handle_socket(socket, addr))
}

/// Actual websocket statemachine (one will be spawned per connection)
async fn handle_socket(mut socket: WebSocket, who: SocketAddr) {
    // send a ping (unsupported by some browsers) just to kick things off and provoke a response.
    if socket
        .send(Message::Ping(Bytes::from_static(&[1, 2, 3])))
        .await
        .is_err()
    {
        return;
    }

    let (mut sender, mut receiver) = socket.split();

    // Process messages in a loop so that we can use `sender` when needed.
    while let Some(msg_result) = receiver.next().await {
        match msg_result {
            Ok(msg) => {
                if let ControlFlow::Break(()) =
                    process_message_with_sender(msg, who, &mut sender).await
                {
                    // Break on a "close" or if sending to the client fails.
                    break;
                }
            }
            Err(e) => {
                break;
            }
        }
    }
}

/// Processes messages received in the websocket. In the case of text messages,
/// it attempts to parse the text as a RunnerPayload. On success, it calls
/// `runner_handler2` to process it and then sends the returned RunnerResult
/// back to the client as a JSON string.
async fn process_message_with_sender(
    msg: Message,
    who: SocketAddr,
    sender: &mut (impl SinkExt<Message> + Unpin),
) -> ControlFlow<(), ()> {
    match msg {
        Message::Text(t) => {
            // Try to parse the message as a RunnerPayload.
            match serde_json::from_str::<RunnerPayload>(&t) {
                Ok(runner_payload) => {
                    let temp_folder_path = create_temp_folder();

                    let proof_required = runner_payload.proof_required.unwrap_or(false);

                    match timeout(
                        Duration::from_secs(30),
                        runner_handler(temp_folder_path.clone(), runner_payload),
                    )
                    .await
                    {
                        Ok(inner_result) => match inner_result {
                            Ok(runner_result) => {
                                if let Ok(result_json) = serde_json::to_string(
                                    &ServerMessage::RunnerResult(runner_result),
                                ) {
                                    if sender
                                        .send(Message::Text(result_json.into()))
                                        .await
                                        .is_err()
                                    {
                                        fs::remove_dir_all(&temp_folder_path)
                                            .expect("Failed to delete temporary folder");
                                        return ControlFlow::Break(());
                                    }
                                }

                                if proof_required {
                                    match timeout(
                                        Duration::from_secs(30),
                                        spawn_blocking({
                                            let temp_folder_path_clone = temp_folder_path.clone();
                                            move || prove_and_verify(&temp_folder_path_clone)
                                        }),
                                    )
                                    .await
                                    {
                                        Ok(join_handle) => match join_handle {
                                            Ok(prove_result) => match prove_result {
                                                Ok(prover_result) => {
                                                    if let Ok(result_json) = serde_json::to_string(
                                                        &ServerMessage::ProverResult(prover_result),
                                                    ) {
                                                        if sender
                                                            .send(Message::Text(result_json.into()))
                                                            .await
                                                            .is_err()
                                                        {
                                                            fs::remove_dir_all(&temp_folder_path)
                                                                        .expect("Failed to delete temporary folder");
                                                            return ControlFlow::Break(());
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    let error_message =
                                                        ServerMessage::ProverAndVerifierError(
                                                            format!(
                                                                "Failed to prove and verify: {:?}",
                                                                e
                                                            ),
                                                        );
                                                    if let Ok(error_json) =
                                                        serde_json::to_string(&error_message)
                                                    {
                                                        let _ = sender
                                                            .send(Message::Text(error_json.into()))
                                                            .await;
                                                    }
                                                    fs::remove_dir_all(&temp_folder_path).expect(
                                                        "Failed to delete temporary folder",
                                                    );
                                                    return ControlFlow::Break(());
                                                }
                                            },
                                            Err(e) => {
                                                fs::remove_dir_all(&temp_folder_path)
                                                    .expect("Failed to delete temporary folder");
                                                return ControlFlow::Break(());
                                            }
                                        },
                                        Err(_) => {
                                            let timeout_error = ServerMessage::ProverAndVerifierError(
                                                "Timeout: prove_and_verify took more than 30 seconds".to_string(),
                                            );
                                            if let Ok(error_json) =
                                                serde_json::to_string(&timeout_error)
                                            {
                                                let _ = sender
                                                    .send(Message::Text(error_json.into()))
                                                    .await;
                                            }
                                            fs::remove_dir_all(&temp_folder_path)
                                                .expect("Failed to delete temporary folder");
                                            return ControlFlow::Break(());
                                        }
                                    }
                                }

                                fs::remove_dir_all(&temp_folder_path)
                                    .expect("Failed to delete temporary folder");
                            }
                            Err(e) => {
                                fs::remove_dir_all(&temp_folder_path)
                                    .expect("Failed to delete temporary folder");
                                return ControlFlow::Break(());
                            }
                        },
                        Err(_) => {
                            let timeout_error = ServerMessage::CompilerAndRunnerError(
                                "Timeout: runner_handler took more than 30 seconds".to_string(),
                            );
                            if let Ok(error_json) = serde_json::to_string(&timeout_error) {
                                let _ = sender.send(Message::Text(error_json.into())).await;
                            }
                            fs::remove_dir_all(&temp_folder_path)
                                .expect("Failed to delete temporary folder");
                            return ControlFlow::Break(());
                        }
                    }
                }
                Err(_e) => {}
            }
        }
        Message::Close(_c) => {
            return ControlFlow::Break(());
        }
        Message::Ping(v) => {
            if sender.send(Message::Pong(v)).await.is_err() {
                return ControlFlow::Break(());
            }
        }
        _ => {}
    }
    ControlFlow::Continue(())
}

pub async fn runner_handler(
    temp_folder_path: PathBuf,
    payload: RunnerPayload,
) -> Result<RunnerResult, ResponseError> {
    let file_path = write_to_file(&temp_folder_path, "main.cairo", &payload.cairo_program_code);
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
        temp_folder_path.clone()
    };

    let runner_result = match run(
        runner_path,
        user_args,
        payload.proof_required.unwrap_or(false),
    ) {
        Ok(result) => result,
        Err(e) => {
            return Err(e);
        }
    };
    Ok(runner_result)
}
