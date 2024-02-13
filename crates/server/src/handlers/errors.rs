use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use cairo1_run::{Error, CAIRO_LANG_COMPILER_VERSION};
use serde::Serialize;

#[derive(Serialize)]
enum LogType {
    Error,
    Warn,
    Info,
}

#[derive(Serialize)]
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

#[derive(Serialize)]
/// Server JSON serializable error type
pub(crate) struct ResponseError {
    #[serde(skip)]
    status_code: StatusCode,
    logs: Vec<LogEntry>,
    cairo_lang_compiler_version: String,
}

impl ResponseError {
    /// Creates new response type with default status code and cairo version
    fn new(errors: Vec<LogEntry>) -> Self {
        Self {
            status_code: StatusCode::EXPECTATION_FAILED,
            logs: errors,
            cairo_lang_compiler_version: CAIRO_LANG_COMPILER_VERSION.to_string(),
        }
    }

    /// Converts cairo 1 error type to a ResponseError
    pub(crate) fn get_error(error: Error) -> Self {
        match error {
            Error::DiagnosticsError(diagnostics) => build_diagnostics_response_error(diagnostics),
            Error::ArgumentsSizeMismatch { expected, actual } => {
                ResponseError::new(vec![LogEntry::new(
                    LogType::Error,
                    format!(
                        "invalid argument count: expected {}, found {}",
                        expected, actual
                    ),
                )])
            }
            _ => ResponseError::new(vec![LogEntry::default()]),
        }
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
    ResponseError::new(errors)
}
