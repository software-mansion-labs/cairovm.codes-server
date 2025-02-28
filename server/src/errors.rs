use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use cairo_lang_compiler::project::ProjectError;
use cairo_lang_runner::RunnerError;
use cairo_vm::air_public_input::PublicInputError;
use cairo_vm::cairo_run::EncodeTraceError;
use cairo_vm::types::errors::program_errors::ProgramError;
use cairo_vm::vm::errors::cairo_run_errors::CairoRunError;
use cairo_vm::vm::errors::trace_errors::TraceError;
use cairo_vm::vm::errors::vm_errors::VirtualMachineError;
// use cairo1_run::{Error, CAIRO_LANG_COMPILER_VERSION};
use serde::Serialize;
use thiserror::Error;

use crate::CAIRO_LANG_COMPILER_VERSION;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to extract arguments from the provided string")]
    BadArgumentStringFormat,
    #[error("Failed to interact with the file system")]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    EncodeTrace(#[from] EncodeTraceError),
    #[error(transparent)]
    VirtualMachine(#[from] VirtualMachineError),
    #[error(transparent)]
    Trace(#[from] TraceError),
    #[error(transparent)]
    PublicInput(#[from] PublicInputError),
    #[error(transparent)]
    Runner(#[from] RunnerError),
    #[error(transparent)]
    ProjectError(#[from] ProjectError),
    // #[error(transparent)]
    // ProgramRegistry(#[from] Box<ProgramRegistryError>),
    // #[error(transparent)]
    // Compilation(#[from] Box<CompilationError>),
    // #[error("Failed to compile to sierra:\n {0}")]
    // SierraCompilation(String),
    // #[error(transparent)]
    // Metadata(#[from] MetadataError),
    #[error(transparent)]
    Program(#[from] ProgramError),
    // #[error(transparent)]
    // Memory(#[from] MemoryError),
    // #[error("Program panicked with {0:?}")]
    // RunPanic(Vec<Felt252>),
    // #[error("Function signature has no return types")]
    // NoRetTypesInSignature,
    // #[error("No size for concrete type id: {0}")]
    // NoTypeSizeForId(ConcreteTypeId),
    // #[error("Concrete type id has no debug name: {0}")]
    // TypeIdNoDebugName(ConcreteTypeId),
    // #[error("No info in sierra program registry for concrete type id: {0}")]
    // NoInfoForType(ConcreteTypeId),
    #[error("Failed to extract return values from VM")]
    FailedToExtractReturnValues,
    #[error("Function expects arguments of size {expected} and received {actual} instead.")]
    ArgumentsSizeMismatch { expected: i16, actual: i16 },
    #[error("Function param {param_index} only partially contains argument {arg_index}.")]
    ArgumentUnaligned {
        param_index: usize,
        arg_index: usize,
    },
    // TODO: find a better error form
    #[error("Compilation error")]
    DiagnosticsError(Vec<String>),
    #[error("Error occurred: {0}")]
    Anyhow(#[from] anyhow::Error),
    #[error(transparent)]
    CairoRunError(#[from] CairoRunError),
}

#[derive(Serialize, Debug)]
enum LogType {
    Error,
    Warn,
    Info,
}

#[derive(Serialize, Debug)]
/// Self contained log entry
pub(crate) struct LogEntry {
    log_type: LogType,
    message: String,
}

impl LogEntry {
    fn new(log_type: LogType, message: String) -> Self {
        Self { log_type, message }
    }
}

impl Default for LogEntry {
    fn default() -> Self {
        Self {
            log_type: LogType::Error,
            message: "failed to compile and run cairo program".to_string(),
        }
    }
}

#[derive(Serialize, Debug)]
/// Server JSON serializable error type
pub struct ResponseError {
    #[serde(skip)]
    status_code: StatusCode,
    is_compilation_successful: bool,
    logs: Vec<LogEntry>,
    cairo_lang_compiler_version: String,
}

impl ResponseError {
    /// Creates new response type with default status code and cairo version
    fn new(errors: Vec<LogEntry>, is_compilation_successful: bool) -> Self {
        Self {
            status_code: StatusCode::EXPECTATION_FAILED,
            is_compilation_successful,
            logs: errors,
            cairo_lang_compiler_version: CAIRO_LANG_COMPILER_VERSION.to_string(),
        }
    }

    /// Converts cairo 1 error type to a ResponseError
    pub(crate) fn get_error(error: Error) -> Self {
        match error {
            Error::DiagnosticsError(diagnostics) => build_diagnostics_response_error(diagnostics),
            Error::ArgumentsSizeMismatch { expected, actual } => ResponseError::new(
                vec![LogEntry::new(
                    LogType::Error,
                    format!(
                        "invalid argument count: expected {}, found {}",
                        expected, actual
                    ),
                )],
                false,
            ),
            Error::CairoRunError(error) => ResponseError::new(
                vec![LogEntry::new(LogType::Error, error.to_string())],
                true,
            ),
            _ => ResponseError::new(vec![LogEntry::default()], false),
        }
    }

    //Converts std::io::error to ResponseError
    pub(crate) fn get_error_from_io(error: std::io::Error) -> Self {
        ResponseError::new(vec![LogEntry::new(LogType::Error, error.to_string())], false)
    }
}

impl IntoResponse for ResponseError {
    fn into_response(self) -> Response {
        (self.status_code, Json(self)).into_response()
    }
}

/// Convert diagnostics to log entry
pub(crate) fn build_log_entry_from_diagnostics(diagnostics: Vec<String>) -> Vec<LogEntry> {
    diagnostics
        .into_iter()
        .map(|message| {
            let error_type = if message.starts_with("error") {
                LogType::Error
            } else if message.starts_with("warning") {
                LogType::Warn
            } else {
                LogType::Info
            };
            LogEntry::new(error_type, message)
        })
        .collect()
}

/// Builds response error from a set of diagnostics strings
fn build_diagnostics_response_error(diagnostics: Vec<String>) -> ResponseError {
    let errors = build_log_entry_from_diagnostics(diagnostics);
    ResponseError::new(errors, false)
}
