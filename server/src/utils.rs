use std::{collections::HashMap, env, fs, path::PathBuf};

use cairo_lang_runner::Arg;
use cairo_lang_sierra::program::Program as SierraProgram;
use cairo_lang_sierra_to_casm::compiler::CairoProgramDebugInfo;
use cairo_vm::Felt252;
use serde::Serialize;
use uuid::Uuid;

pub fn write_to_temp_file(content: &str) -> (PathBuf, PathBuf) {
    let current_dir = env::current_dir().expect("Failed to get current directory");
    let uuid = Uuid::new_v4();
    let folder_name = uuid.to_string();
    let parent_dir = current_dir.join(&folder_name);
    if !parent_dir.exists() {
        fs::create_dir_all(&parent_dir).expect("failed to create new folder");
    }
    let file_path = parent_dir.join("main.cairo");
    fs::write(&file_path, content).expect("Failed to write to file");
    (file_path, parent_dir)
}

pub fn process_args(value: &str) -> Result<Vec<Arg>, String> {
    if value.is_empty() {
        return Ok(Vec::new());
    }

    let mut args = Vec::new();
    let mut input = value.split(' ');

    while let Some(value) = input.next() {
        // First argument in an array
        if value.starts_with('[') {
            let mut array_arg = vec![Arg::from(
                Felt252::from_dec_str(value.strip_prefix('[').unwrap())
                    .map_err(|e| e.to_string())?,
            )];

            // Process following args in array
            let mut array_end = false;
            while !array_end {
                if let Some(value) = input.next() {
                    // Last arg in array
                    if value.ends_with(']') {
                        array_arg.push(Arg::from(
                            Felt252::from_dec_str(value.strip_suffix(']').unwrap())
                                .map_err(|e| e.to_string())?,
                        ));
                        array_end = true;
                    } else {
                        array_arg.push(Arg::from(
                            Felt252::from_dec_str(value).map_err(|e| e.to_string())?,
                        ));
                    }
                }
            }
            // Finalize array
            args.push(Arg::Array(array_arg));
        } else {
            // Single argument
            args.push(Arg::from(
                Felt252::from_dec_str(value).map_err(|e| e.to_string())?,
            ));
        }
    }

    Ok(args)
}

pub fn make_casm_to_sierra_map(
    debug_info: &CairoProgramDebugInfo,
    casm_headers_len: usize,
) -> HashMap<usize, Vec<usize>> {
    let mut map: HashMap<usize, Vec<usize>> = HashMap::new();
    let sierra_statement_info_len = debug_info.sierra_statement_info.len();
    for (i, sierra_info) in debug_info
        .sierra_statement_info
        .iter()
        .enumerate()
        .take(sierra_statement_info_len - 1)
    {
        let key = sierra_info.instruction_idx + casm_headers_len;
        map.entry(key).or_insert_with(Vec::new).push(i);
    }
    map
}

#[derive(Serialize)]
pub struct SierraFormattedProgram {
    pub type_declarations: Vec<String>,
    pub libfunc_declarations: Vec<String>,
    pub statements: Vec<String>,
    pub funcs: Vec<String>,
}

pub fn format_sierra_program(sierra_program: SierraProgram) -> SierraFormattedProgram {
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
            .enumerate()
            .map(|(index, statement)| format!("{} // {}", statement.to_string(), index))
            .collect(),
        funcs: sierra_program
            .funcs
            .iter()
            .map(|func| func.to_string())
            .collect(),
    }
}
