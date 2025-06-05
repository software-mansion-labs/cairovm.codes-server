use anyhow::Result;
use cairo_lang_compiler::{
    db::RootDatabase, diagnostics::DiagnosticsReporter, project::setup_project,
};
use cairo_lang_diagnostics::{FormattedDiagnosticEntry, ToOption};
use cairo_lang_filesystem::cfg::{Cfg, CfgSet};
use cairo_lang_runnable_utils::builder::{EntryCodeConfig, RunnableBuilder};
use cairo_lang_runner::{casm_run::format_next_item, Arg, SierraCasmRunner, StarknetState};
use cairo_lang_sierra_generator::replace_ids::{DebugReplacer, SierraIdReplacer};
use cairo_lang_sierra_generator::{db::SierraGenGroup, program_generator::SierraProgramWithDebug};
use cairo_lang_starknet::{
    contract::{find_contracts, get_contracts_info},
    starknet_plugin_suite,
};
use cairo_lang_test_plugin::test_plugin_suite;
use cairo_lang_utils::Upcast;
use std::collections::HashMap;
use std::{path::PathBuf, sync::Arc, usize};

use crate::{
    errors::{build_log_entry_from_diagnostics, Error},
    runner_utils::run_function_with_starknet_context,
    tracer::{
        sierra_to_cairo::SierraToCairoDebugInfo,
        trace::{make_trace_data, TracerData},
    },
    utils::{format_sierra_program, make_casm_to_sierra_map, SierraFormattedProgram},
    ResponseError, RunnerResult, CAIRO_LANG_COMPILER_VERSION,
};

pub fn run(
    project_path: PathBuf,
    user_args: Vec<Arg>,
    _proof_required: bool,
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
    } = match db.get_sierra_program(main_crate_ids.clone()).to_option() {
        Some(program) => Arc::unwrap_or_clone(program),
        None => {
            return Ok(RunnerResult {
                cairo_lang_compiler_version: CAIRO_LANG_COMPILER_VERSION.to_string(),
                serialized_output: None,
                stdout_captured: None,
                execution_panic_message: None,
                is_compilation_successful: false,
                is_execution_successful: false,
                tracer_data: TracerData {
                    pc_inst_map: HashMap::new(),
                    trace: vec![],
                    memory: HashMap::new(),
                    pc_to_inst_indexes_map: HashMap::new(),
                    callstack: vec![],
                    trace_entries_to_sierra_vars: vec![],
                    sierra_to_cairo_debug_info: SierraToCairoDebugInfo {
                        sierra_statements_to_cairo_info: HashMap::new(),
                    },
                },
                casm_formatted_instructions: vec![],
                casm_to_sierra_map: HashMap::new(),
                sierra_formatted_program: SierraFormattedProgram {
                    type_declarations: vec![],
                    libfunc_declarations: vec![],
                    statements: vec![],
                    funcs: vec![],
                },
                logs: build_log_entry_from_diagnostics(program_diagnostics),
                compilation_time_ms: 0,
                execution_time_ms: 0,
                proving_is_not_supported: true,
                proof_required: false,
            });
        }
    };

    let replacer = DebugReplacer { db };
    replacer.enrich_function_names(&mut sierra_program);

    let contracts = find_contracts((*db).upcast(), &main_crate_ids);
    let contracts_info = match get_contracts_info(db, contracts, &replacer) {
        Ok(info) => info,
        Err(_) => {
            return Ok(RunnerResult {
                cairo_lang_compiler_version: CAIRO_LANG_COMPILER_VERSION.to_string(),
                serialized_output: None,
                stdout_captured: None,
                execution_panic_message: None,
                is_compilation_successful: false,
                is_execution_successful: false,
                tracer_data: TracerData {
                    pc_inst_map: HashMap::new(),
                    trace: vec![],
                    memory: HashMap::new(),
                    pc_to_inst_indexes_map: HashMap::new(),
                    callstack: vec![],
                    trace_entries_to_sierra_vars: vec![],
                    sierra_to_cairo_debug_info: SierraToCairoDebugInfo {
                        sierra_statements_to_cairo_info: HashMap::new(),
                    },
                },
                casm_formatted_instructions: vec![],
                casm_to_sierra_map: HashMap::new(),
                sierra_formatted_program: SierraFormattedProgram {
                    type_declarations: vec![],
                    libfunc_declarations: vec![],
                    statements: vec![],
                    funcs: vec![],
                },
                logs: build_log_entry_from_diagnostics(program_diagnostics),
                compilation_time_ms: 0,
                execution_time_ms: 0,
                proving_is_not_supported: true,
                proof_required: false,
            });
        }
    };

    let sierra_program = replacer.apply(&sierra_program);

    let runner = SierraCasmRunner::new(sierra_program.clone(), None, contracts_info, None)
        .map_err(|err| ResponseError::get_error(Error::Runner(err)))?;

    let builder = RunnableBuilder::new(sierra_program.clone(), None).unwrap();

    let func = runner
        .find_function("::main")
        .map_err(|_| ResponseError::get_error(Error::MainNotFound))?;

    let (result, relocated_trace, stdout_data) = run_function_with_starknet_context(
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
            serialized_output = Some(
                values
                    .into_iter()
                    .map(|v| {
                        // Convert each value to an integer (assuming the conversion is available)
                        let int_val: u128 = v.try_into().unwrap();
                        int_val.to_string()
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        cairo_lang_runner::RunResultValue::Panic(values) => {
            let mut message: String = "Run panicked with [".to_string();
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
        stdout_captured: Some(stdout_data),
        execution_panic_message: execution_panic_message.clone(),
        is_compilation_successful: true,
        is_execution_successful: execution_panic_message.is_none(),
        tracer_data,
        casm_formatted_instructions,
        casm_to_sierra_map,
        sierra_formatted_program,
        logs: build_log_entry_from_diagnostics(program_diagnostics),
        compilation_time_ms: 0,
        execution_time_ms: 0,
        proving_is_not_supported: true,
        proof_required: false,
    })
}
