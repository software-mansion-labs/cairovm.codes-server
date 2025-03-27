use anyhow::{Context, Result};
use bincode::enc::write::Writer;
use cairo_lang_compiler::{
    db::RootDatabase, diagnostics::DiagnosticsReporter, project::setup_project,
};
use cairo_lang_diagnostics::FormattedDiagnosticEntry;
use cairo_lang_executable::{
    compile::ExecutableConfig,
    executable::{EntryPointKind, Executable},
    plugin::executable_plugin_suite,
};
use cairo_lang_filesystem::cfg::{Cfg, CfgSet};
use cairo_lang_runner::{Arg, CairoHintProcessor, build_hints_dict};
use cairo_lang_sierra_generator::program_generator::SierraProgramWithDebug;
use cairo_lang_sierra_generator::replace_ids::SierraIdReplacer;
use cairo_lang_starknet::starknet_plugin_suite;
use cairo_lang_test_plugin::test_plugin_suite;
use cairo_vm::{
    Felt252,
    cairo_run::{self, CairoRunConfig},
    types::{program::Program, relocatable::MaybeRelocatable},
};
use cairo_vm::{cairo_run::cairo_run_program, types::layout_name::LayoutName};
use std::{
    io::{self, Write},
    path::PathBuf,
};

use crate::{
    CAIRO_LANG_COMPILER_VERSION, ResponseError, RunnerResult,
    compilation::compile_executable_in_prepared_db,
    errors::{Error, build_log_entry_from_diagnostics},
    tracer::trace::make_trace_data,
    utils::{format_sierra_program, make_casm_to_sierra_map},
};

pub fn run(
    project_path: PathBuf,
    user_args: Vec<Arg>,
    proof_required: bool,
) -> Result<RunnerResult, ResponseError> {
    let mut program_diagnostics: Vec<String> = Vec::new();
    let diagnostics_reporter =
        DiagnosticsReporter::callback(|diagnostic: FormattedDiagnosticEntry| {
            program_diagnostics.push(format!(
                "{}: {}",
                diagnostic.severity(),
                diagnostic.message()
            ));
        })
        .allow_warnings();

    let compile_start_time = std::time::Instant::now(); // Start measuring compilation time

    let mut db = RootDatabase::builder()
        .skip_auto_withdraw_gas()
        .with_cfg(CfgSet::from_iter([
            Cfg::kv("target", "test"),
            Cfg::kv("gas", "disabled"),
        ]))
        .detect_corelib()
        .with_default_plugin_suite(executable_plugin_suite())
        .with_default_plugin_suite(test_plugin_suite())
        .with_default_plugin_suite(starknet_plugin_suite())
        .build()
        .map_err(|err| ResponseError::get_error(Error::Anyhow(err)))?;

    let main_crate_ids = setup_project(&mut db, project_path.join("main.cairo").as_ref())
        .map_err(|err| ResponseError::get_error(Error::ProjectError(err)))?;

    let (compiled_function, sierra_program, sierra_debug_info) =
        match compile_executable_in_prepared_db(
            &db,
            None,
            main_crate_ids,
            diagnostics_reporter,
            ExecutableConfig {
                allow_syscalls: false,
            },
        ) {
            Ok(result) => result,
            Err(_err) => {
                return Err(ResponseError::get_error(Error::DiagnosticsError(
                    program_diagnostics,
                )));
            }
        };

    let sierra_program =
        cairo_lang_sierra_generator::replace_ids::DebugReplacer { db: &db }.apply(&sierra_program);

    let compilation_time = compile_start_time.elapsed(); // Measure compilation time

    let casm_to_sierra_map = make_casm_to_sierra_map(
        &compiled_function.program.debug_info,
        compiled_function.wrapper.header.len(),
    );

    let casm_formatted_instructions: Vec<String> = compiled_function
        .program
        .instructions
        .iter()
        .map(|instruction| instruction.to_string())
        .collect();

    let sierra_formatted_program = format_sierra_program(sierra_program.clone());

    let casm_program = compiled_function.program.clone();

    let executable = Executable::new(compiled_function);

    let data = executable
        .program
        .bytecode
        .iter()
        .map(Felt252::from)
        .map(MaybeRelocatable::from)
        .collect();

    let (hints, string_to_hint) = build_hints_dict(&executable.program.hints);

    let entrypoint = executable
        .entrypoints
        .iter()
        .find(|e| matches!(e.kind, EntryPointKind::Standalone))
        .with_context(|| "No `Standalone` entrypoint found.")
        .map_err(|err| ResponseError::get_error(Error::Anyhow(err)))?;

    let program = Program::new_for_proof(
        entrypoint.builtins.clone(),
        data,
        entrypoint.offset,
        entrypoint.offset + 4,
        hints,
        Default::default(),
        Default::default(),
        vec![],
        None,
    )
    .map_err(|err| ResponseError::get_error(Error::Program(err)))?;

    let mut hint_processor = CairoHintProcessor {
        runner: None,
        user_args: vec![vec![Arg::Array(user_args)]],
        string_to_hint,
        starknet_state: Default::default(),
        run_resources: Default::default(),
        syscalls_used_resources: Default::default(),
        no_temporary_segments: false,
        markers: Default::default(),
    };

    let cairo_run_config = CairoRunConfig {
        trace_enabled: true,
        relocate_mem: true,
        layout: LayoutName::all_cairo,
        proof_mode: true,
        secure_run: Some(false),
        allow_missing_builtins: Some(false),
        ..Default::default()
    };

    let run_start_time = std::time::Instant::now(); // Start measuring running time
    let mut runner = cairo_run_program(&program, &cairo_run_config, &mut hint_processor)
        .map_err(|err| ResponseError::get_error(Error::CairoRunError(err)))?;

    let running_time = run_start_time.elapsed(); // Measure running time

    let mut serialized_output = String::new();
    runner
        .vm
        .write_output(&mut serialized_output)
        .map_err(|err| ResponseError::get_error(Error::VirtualMachine(err)))?;

    let execution_panic_message = None;

    // Remove any trailing newline from the output.
    serialized_output = serialized_output.trim_end_matches('\n').to_string();

    let trace_path = project_path.join("trace.bin");

    let relocated_trace = runner
        .relocated_trace
        .as_ref()
        .with_context(|| "Trace not relocated.")
        .map_err(|err| ResponseError::get_error(Error::Anyhow(err)))?;
    let mut writer = FileWriter::new(3 * 1024 * 1024, &trace_path)
        .map_err(|err| ResponseError::get_error(Error::Anyhow(err)))?;
    cairo_run::write_encoded_trace(relocated_trace, &mut writer)
        .map_err(|err| ResponseError::get_error(Error::EncodeTrace(err)))?;
    writer
        .flush()
        .map_err(|err| ResponseError::get_error(Error::Anyhow(anyhow::Error::from(err))))?;

    let memory_path = project_path.join("memory.bin");

    let mut writer = FileWriter::new(5 * 1024 * 1024, &memory_path)
        .map_err(|err| ResponseError::get_error(Error::Anyhow(err)))?;
    cairo_run::write_encoded_memory(&runner.relocated_memory, &mut writer)
        .map_err(|err| ResponseError::get_error(Error::EncodeTrace(err)))?;
    writer
        .flush()
        .map_err(|err| ResponseError::get_error(Error::Anyhow(anyhow::Error::from(err))))?;

    let air_public_input_path = project_path.join("air_public_input.txt");

    let json = runner
        .get_air_public_input()
        .map_err(|err| ResponseError::get_error(Error::Anyhow(anyhow::Error::from(err))))?
        .serialize_json()
        .map_err(|err| ResponseError::get_error(Error::Anyhow(anyhow::Error::from(err))))?;
    std::fs::write(air_public_input_path, json)
        .map_err(|err| ResponseError::get_error(Error::Anyhow(anyhow::Error::from(err))))?;

    let absolute = |path_buf: PathBuf| {
        path_buf
            .as_path()
            .canonicalize()
            .unwrap_or(path_buf)
            .to_string_lossy()
            .to_string()
    };

    let air_private_input_path = project_path.join("air_private_input.txt");

    let json = runner
        .get_air_private_input()
        .to_serializable(absolute(trace_path), absolute(memory_path))
        .serialize_json()
        .map_err(|err| ResponseError::get_error(Error::Anyhow(anyhow::Error::from(err))))?;
    std::fs::write(air_private_input_path, json)
        .map_err(|err| ResponseError::get_error(Error::Anyhow(anyhow::Error::from(err))))?;

    let tracer_data = match make_trace_data(
        relocated_trace.clone(),
        runner.relocated_memory,
        &casm_program.debug_info,
        &casm_to_sierra_map,
        &SierraProgramWithDebug {
            program: sierra_program,
            debug_info: sierra_debug_info,
        },
        &db,
    ) {
        Ok(result) => result,
        Err(error) => {
            return Err(ResponseError::get_error(Error::Anyhow(
                anyhow::Error::from(error),
            )));
        }
    };

    let is_execution_successful = execution_panic_message.is_none();

    let compilation_time_ms = compilation_time.as_millis() as u64;
    let execution_time_ms = running_time.as_millis() as u64;

    Ok(RunnerResult {
        cairo_lang_compiler_version: CAIRO_LANG_COMPILER_VERSION.to_string(),
        serialized_output: if serialized_output.is_empty() {
            None
        } else {
            Some(serialized_output)
        },
        execution_panic_message,
        is_compilation_successful: true,
        is_execution_successful,
        tracer_data,
        casm_formatted_instructions,
        casm_to_sierra_map,
        sierra_formatted_program,
        logs: build_log_entry_from_diagnostics(program_diagnostics),
        compilation_time_ms,
        execution_time_ms,
        proving_is_not_supported: false,
        proof_required,
    })
}

/// Writer implementation for a file.
struct FileWriter {
    buf_writer: io::BufWriter<std::fs::File>,
    bytes_written: usize,
}

impl Writer for FileWriter {
    fn write(&mut self, bytes: &[u8]) -> Result<(), bincode::error::EncodeError> {
        self.buf_writer
            .write_all(bytes)
            .map_err(|e| bincode::error::EncodeError::Io {
                inner: e,
                index: self.bytes_written,
            })?;

        self.bytes_written += bytes.len();

        Ok(())
    }
}

impl FileWriter {
    /// Create a new instance of `FileWriter` with the given file path.
    fn new(capacity: usize, path: &PathBuf) -> anyhow::Result<Self> {
        Ok(Self {
            buf_writer: io::BufWriter::with_capacity(capacity, std::fs::File::create(path)?),
            bytes_written: 0,
        })
    }

    /// Flush the writer.
    ///
    /// Would automatically be called when the writer is dropped, but errors are ignored in that
    /// case.
    fn flush(&mut self) -> io::Result<()> {
        self.buf_writer.flush()
    }
}
