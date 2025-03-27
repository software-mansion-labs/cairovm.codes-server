use anyhow::{Context, Result};
use bincode::enc::write::Writer;
use cairo_lang_compiler::{
    db::RootDatabase, diagnostics::DiagnosticsReporter, project::setup_project,
};
use cairo_lang_diagnostics::{FormattedDiagnosticEntry, ToOption};
use cairo_lang_executable::{
    compile::ExecutableConfig,
    executable::{EntryPointKind, Executable},
    plugin::executable_plugin_suite,
};
use cairo_lang_filesystem::cfg::{Cfg, CfgSet};
use cairo_lang_runnable_utils::builder::{EntryCodeConfig, RunnableBuilder};
use cairo_lang_runner::{
    Arg, CairoHintProcessor, RunResult, RunnerError, SierraCasmRunner, StarknetState,
    build_hints_dict,
    casm_run::{self, RunFunctionResult, format_next_item},
    initialize_vm,
};
use cairo_lang_sierra::program::{Function, Program as SierraProgram};
use cairo_lang_sierra_generator::replace_ids::{DebugReplacer, SierraIdReplacer};
use cairo_lang_sierra_generator::{db::SierraGenGroup, program_generator::SierraProgramWithDebug};
use cairo_lang_sierra_to_casm::compiler::CairoProgramDebugInfo;
use cairo_lang_starknet::{
    contract::{find_contracts, get_contracts_info},
    starknet_plugin_suite,
};
use cairo_lang_test_plugin::test_plugin_suite;
use cairo_lang_utils::Upcast;
use cairo_vm::{
    Felt252,
    cairo_run::{self, CairoRunConfig},
    hint_processor::hint_processor_definition::HintProcessor,
    serde::deserialize_program::HintParams,
    types::{builtin_name::BuiltinName, program::Program, relocatable::MaybeRelocatable},
};
use cairo_vm::{cairo_run::cairo_run_program, types::layout_name::LayoutName};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    usize,
};

use crate::{
    CAIRO_LANG_COMPILER_VERSION, ResponseError, RunnerResult,
    compilation::compile_executable_in_prepared_db,
    errors::{Error, LogEntry, build_log_entry_from_diagnostics},
    runner_utils::run_function_with_starknet_context,
    tracer::trace::{TracerData, make_trace_data},
    utils::{SierraFormattedProgram, format_sierra_program, make_casm_to_sierra_map},
};

pub fn run(
    project_path: PathBuf,
    user_args: Vec<Arg>,
    proof_required: bool,
    _verification_required: bool,
) -> Result<RunnerResult, ResponseError> {
    println!("Starting a new simple run...");

    let mut db_builder = RootDatabase::builder();
    db_builder.detect_corelib();
    db_builder
        .skip_auto_withdraw_gas()
        .with_cfg(CfgSet::from_iter([
            Cfg::kv("gas", "disabled"),
            Cfg::kv("target", "test"),
        ]))
        .with_default_plugin_suite(test_plugin_suite())
        .with_default_plugin_suite(starknet_plugin_suite());
    let db = &mut db_builder
        .build()
        .map_err(|err| ResponseError::get_error(Error::Anyhow(err)))?;

    let main_crate_ids = setup_project(db, &project_path).map_err(|err| {
        dbg!(&err);
        ResponseError::get_error(Error::ProjectError(err))
    })?;

    let mut program_diagnostics: Vec<String> = Vec::new();
    let mut diagnostics_reporter =
        DiagnosticsReporter::callback(|diagnostic: FormattedDiagnosticEntry| {
            println!("{}: {}", diagnostic.severity(), diagnostic.message());
            program_diagnostics.push(format!(
                "{}: {}",
                diagnostic.severity(),
                diagnostic.message()
            ));
        })
        .allow_warnings();

    diagnostics_reporter.check(db);
    drop(diagnostics_reporter); // Drop the reporter, releasing the borrow.

    // if !diagnostics_ok {
    //     println!("diagnostics_ok {:?}", diagnostics_ok);
    //     return Err(ResponseError::get_error(Error::DiagnosticsError(
    //         program_diagnostics,
    //     )));
    // }

    let SierraProgramWithDebug {
        program: mut sierra_program,
        debug_info,
    } = Arc::unwrap_or_clone(
        db.get_sierra_program(main_crate_ids.clone())
            .to_option()
            .with_context(|| "Compilation failed without any diagnostics.")
            .map_err(|err| {
                ResponseError::get_error(Error::DiagnosticsError(program_diagnostics))
            })?,
    );

    let replacer = DebugReplacer { db };
    replacer.enrich_function_names(&mut sierra_program);

    let contracts = find_contracts((*db).upcast(), &main_crate_ids);
    let contracts_info = get_contracts_info(db, contracts, &replacer)
        .map_err(|err| ResponseError::get_error(Error::Anyhow(err)))?;
    let sierra_program = replacer.apply(&sierra_program);

    let runner = SierraCasmRunner::new(sierra_program.clone(), None, contracts_info, None)
        .map_err(|err| ResponseError::get_error(Error::Runner(err)))?;

    let builder = RunnableBuilder::new(sierra_program.clone(), None).unwrap();

    let func = runner
        .find_function("::main")
        .map_err(|err| ResponseError::get_error(Error::Runner(err)))?;

    let (result, relocated_trace) = run_function_with_starknet_context(
        &runner,
        &builder,
        func,
        user_args,
        Some(usize::MAX),
        StarknetState::default(),
    )
    .map_err(|err| ResponseError::get_error(Error::Runner(err)))?;

    let mut serialized_output: Option<String> = None;
    let mut execution_panic_message: Option<String> = None;

    match result.value {
        cairo_lang_runner::RunResultValue::Success(values) => {
            // println!("Run completed successfully, returning {values:?}");
            serialized_output = Some(
                values.into_iter()
                      .map(|v| {
                          // Convert each value to an integer (assuming the conversion is available)
                          let int_val: u128 = v.try_into().unwrap();
                          int_val.to_string()
                      })
                      .collect::<Vec<_>>()
                      .join(", ")
            );
        }
        cairo_lang_runner::RunResultValue::Panic(values) => {
            let mut message: String = "Run panicked with [".to_string();
            // print!("Run panicked with [");
            let mut felts = values.into_iter();
            let mut first = true;
            while let Some(item) = format_next_item(&mut felts) {
                if !first {
                    message.push_str(", ");
                }
                first = false;
                message.push_str(&item.quote_if_string());
            }
            message.push_str("].");
            execution_panic_message = Some(message);
        }
    };

    let wrapper_info = builder
        .create_wrapper_info(func, EntryCodeConfig::testing())
        .unwrap();

    let casm_to_sierra_map = make_casm_to_sierra_map(
        &builder.casm_program().debug_info,
        wrapper_info.header.len(),
    );

    let tracer_data = match make_trace_data(
        relocated_trace,
        result.memory,
        &builder.casm_program().debug_info,
        &casm_to_sierra_map,
        &SierraProgramWithDebug {
            program: sierra_program.clone(),
            debug_info,
        },
        &db,
    ) {
        Ok(result) => result,
        Err(error) => {
            dbg!(&error);
            // fs::remove_dir_all(&folder_path).expect("Failed to delete temporary folder");
            return Err(ResponseError::get_error(Error::Anyhow(
                anyhow::Error::from(error),
            )));
        }
    };

    let casm_formatted_instructions: Vec<String> = builder
        .casm_program()
        .instructions
        .iter()
        .map(|instruction| instruction.to_string())
        .collect();

    let sierra_formatted_program = format_sierra_program(sierra_program);

    Ok(RunnerResult {
        cairo_lang_compiler_version: CAIRO_LANG_COMPILER_VERSION.to_string(),
        serialized_output,
        execution_panic_message: execution_panic_message.clone(),
        is_compilation_successful: true,
        is_execution_successful: execution_panic_message.is_none(),
        tracer_data,
        casm_formatted_instructions,
        casm_to_sierra_map,
        sierra_formatted_program,
        logs: build_log_entry_from_diagnostics(vec![]),
        proof: None,
        compilation_time_ms: 0,
        execution_time_ms: 0,
        proving_time_ms: None,
        verification_time_ms: None,
        proving_is_not_supported: proof_required,
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
