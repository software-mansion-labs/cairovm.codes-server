use crate::handlers::errors::{build_log_entry_from_diagnostics, LogEntry, ResponseError};
use axum::Json;
use cairo1_run::{run_program_at_path, RunResult, CAIRO_LANG_COMPILER_VERSION};
use cairo_lang_sierra::program::Program;
use cairo_lang_sierra_to_casm::compiler::CairoProgramDebugInfo;
use rand::distributions::{Distribution, Uniform};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, env, fs, path::PathBuf};
use tracer::{make_tracer_data, TracerData};

fn write_to_temp_file(content: &str) -> PathBuf {
    let current_dir = env::current_dir().expect("Failed to get current directory");
    let mut rng = rand::thread_rng();
    let alphabet = Uniform::from('a'..'z');
    let file_name: String = std::iter::repeat_with(|| alphabet.sample(&mut rng))
        .take(6)
        .collect();
    let parent_dir = current_dir.join(file_name);
    if !parent_dir.exists() {
        fs::create_dir_all(&parent_dir).expect("failed to create new folder");
    }
    let file_path = parent_dir.join("main.cairo");
    fs::write(&file_path, content).expect("Failed to write to file");
    file_path
}

#[derive(Serialize)]
pub struct SierraFormattedProgram {
    pub type_declarations: Vec<String>,
    pub libfunc_declarations: Vec<String>,
    pub statements: Vec<String>,
    pub funcs: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct RunnerPayload {
    cairo_program_code: String,
    program_arguments: Option<String>,
}

#[derive(Serialize)]
pub struct RunnerResult {
    sierra_program_code: String,
    casm_program_code: String,
    cairo_lang_compiler_version: String,
    serialized_output: Option<String>,
    tracer_data: TracerData,
    casm_formatted_instructions: Vec<String>,
    casm_to_sierra_map: HashMap<usize, Vec<usize>>,
    sierra_formatted_program: SierraFormattedProgram,
    logs: Vec<LogEntry>,
}

pub async fn runner_handler(
    Json(payload): Json<RunnerPayload>,
) -> Result<Json<RunnerResult>, ResponseError> {
    let file_path = write_to_temp_file(&payload.cairo_program_code);

    let program_arguments = payload.program_arguments.unwrap_or(String::new());

    let RunResult {
        sierra_program,
        casm_program,
        serialized_output,
        trace,
        memory,
        instructions,
        headers_len,
        diagnostics,
    } = match run_program_at_path(&file_path, &program_arguments[..]) {
        Ok(result) => result,
        Err(error) => {
            dbg!(&error);
            fs::remove_file(&file_path).expect("Failed to delete temporary file");
            return Err(ResponseError::get_error(error));
        }
    };

    // Delete the temporary file
    fs::remove_file(&file_path).expect("Failed to delete temporary file");

    let tracer_data = make_tracer_data(trace, memory);

    let casm_program_code = instructions
        .iter()
        .map(|instruction| instruction.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    let casm_formatted_instructions = instructions
        .iter()
        .map(|instruction| instruction.to_string())
        .collect();

    Ok(Json(RunnerResult {
        sierra_program_code: format!("{sierra_program}"),
        casm_program_code,
        cairo_lang_compiler_version: CAIRO_LANG_COMPILER_VERSION.to_string(),
        serialized_output,
        tracer_data,
        casm_formatted_instructions,
        casm_to_sierra_map: make_casm_to_sierra_map(casm_program.debug_info, headers_len),
        sierra_formatted_program: format_sierra_program(sierra_program),
        logs: build_log_entry_from_diagnostics(diagnostics),
    }))
}

fn format_sierra_program(sierra_program: Program) -> SierraFormattedProgram {
    SierraFormattedProgram {
        type_declarations: sierra_program
            .type_declarations
            .iter()
            .map(|type_decl| type_decl.to_string())
            .collect(),
        libfunc_declarations: sierra_program
            .libfunc_declarations
            .iter()
            .map(|libfunc_decl| libfunc_decl.to_string())
            .collect(),
        statements: sierra_program
            .statements
            .iter()
            .map(|statement| statement.to_string())
            .collect(),
        funcs: sierra_program
            .funcs
            .iter()
            .map(|func| func.to_string())
            .collect(),
    }
}

fn make_casm_to_sierra_map(
    debug_info: CairoProgramDebugInfo,
    casm_headers_len: usize,
) -> HashMap<usize, Vec<usize>> {
    let mut map: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, sierra_info) in debug_info.sierra_statement_info.iter().enumerate() {
        let key = sierra_info.instruction_idx + casm_headers_len;
        map.entry(key).or_insert_with(Vec::new).push(i);
    }
    map
}
