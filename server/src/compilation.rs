use std::sync::Arc;

use anyhow::{Context, Result};
use cairo_lang_compiler::{db::RootDatabase, diagnostics::DiagnosticsReporter};
use cairo_lang_debug::debug::DebugWithDb;
use cairo_lang_executable::{
    compile::{CompiledFunction, ExecutableConfig, find_executable_functions},
    plugin::EXECUTABLE_PREFIX,
};
use cairo_lang_filesystem::ids::CrateId;
use cairo_lang_lowering::ids::ConcreteFunctionWithBodyId;
use cairo_lang_runnable_utils::builder::{EntryCodeConfig, RunnableBuilder};
use cairo_lang_sierra::program::Program as SierraProgram;
use cairo_lang_sierra_generator::{
    db::SierraGenGroup,
    program_generator::{SierraProgramDebugInfo, SierraProgramWithDebug},
};
use cairo_lang_utils::Intern;
use cairo_lang_utils::Upcast;
use itertools::Itertools;

/// Returns the path to the function that the executable is wrapping.
///
/// If the executable is not wrapping a function, returns the full path of the executable.
pub fn originating_function_path(db: &RootDatabase, wrapper: ConcreteFunctionWithBodyId) -> String {
    let semantic = wrapper.base_semantic_function(db);
    let wrapper_name = semantic.name(db);
    let wrapper_full_path = semantic.full_path(db.upcast());
    let Some(wrapped_name) = wrapper_name.strip_prefix(EXECUTABLE_PREFIX) else {
        return wrapper_full_path;
    };
    let Some(wrapper_path_to_module) = wrapper_full_path.strip_suffix(wrapper_name.as_str()) else {
        return wrapper_full_path;
    };
    format!("{}{}", wrapper_path_to_module, wrapped_name)
}

/// Runs compiler on the specified executable function.
/// If no executable was specified, verify that there is only one.
/// Otherwise, return an error.
pub fn compile_executable_in_prepared_db(
    db: &RootDatabase,
    executable_path: Option<&str>,
    main_crate_ids: Vec<CrateId>,
    mut diagnostics_reporter: DiagnosticsReporter<'_>,
    config: ExecutableConfig,
) -> Result<(CompiledFunction, SierraProgram, SierraProgramDebugInfo)> {
    let executables = find_executable_functions(db, main_crate_ids, executable_path);

    let executable = match executables.len() {
        0 => {
            // Report diagnostics as they might reveal the reason why no executable was found.
            diagnostics_reporter.ensure(db)?;
            anyhow::bail!("Requested `#[executable]` not found.");
        }
        1 => executables[0],
        _ => {
            let executable_names = executables
                .iter()
                .map(|executable| originating_function_path(db, *executable))
                .join("\n  ");
            anyhow::bail!(
                "More than one executable found in the main crate: \n  {}\nUse --executable to \
                 specify which to compile.",
                executable_names
            );
        }
    };

    compile_executable_function_in_prepared_db(db, executable, diagnostics_reporter, config)
}

/// Runs compiler for an executable function.
///
/// # Arguments
/// * `db` - Preloaded compilation database.
/// * `executable` - [`ConcreteFunctionWithBodyId`]s to compile.
/// * `diagnostics_reporter` - The diagnostics reporter.
/// * `config` - If true, the compilation will not fail if the program is not sound.
/// # Returns
/// * `Ok(Vec<String>)` - The result artifact of the compilation.
/// * `Err(anyhow::Error)` - Compilation failed.
pub fn compile_executable_function_in_prepared_db(
    db: &RootDatabase,
    executable: ConcreteFunctionWithBodyId,
    mut diagnostics_reporter: DiagnosticsReporter<'_>,
    config: ExecutableConfig,
) -> Result<(CompiledFunction, SierraProgram, SierraProgramDebugInfo)> {
    diagnostics_reporter.ensure(db)?;
    let SierraProgramWithDebug {
        program: sierra_program,
        debug_info,
    } = Arc::unwrap_or_clone(
        db.get_sierra_program_for_functions(vec![executable])
            .ok()
            .with_context(|| "Compilation failed without any diagnostics.")?,
    );
    if !config.allow_syscalls {
        // Finding if any syscall libfuncs are used in the program.
        // If any are found, the compilation will fail, as syscalls are not proved in executables.
        for libfunc in &sierra_program.libfunc_declarations {
            if libfunc.long_id.generic_id.0.ends_with("_syscall") {
                anyhow::bail!(
                    "The function is using libfunc `{}`. Syscalls are not supported in \
                     `#[executable]`.",
                    libfunc.long_id.generic_id
                );
            }
        }
    }

    // Since we build the entry point asking for a single function - we know it will be first, and
    // that it will be available.
    let executable_func = sierra_program.funcs[0].clone();
    assert_eq!(
        executable_func.id,
        executable.function_id(db.upcast()).unwrap().intern(db)
    );
    let builder = RunnableBuilder::new(sierra_program.clone(), None).map_err(|err| {
        let mut locs = vec![];
        for stmt_idx in err.stmt_indices() {
            // Note that the `last` is used here as the call site is the most relevant location.
            if let Some(loc) = debug_info
                .statements_locations
                .locations
                .get(&stmt_idx)
                .and_then(|stmt_locs| stmt_locs.last())
            {
                locs.push(format!(
                    "#{stmt_idx} {:?}",
                    loc.diagnostic_location(db).debug(db)
                ))
            }
        }

        anyhow::anyhow!(
            "Failed to create runnable builder: {}\n{}",
            err,
            locs.join("\n")
        )
    })?;

    // If syscalls are allowed it means we allow for unsound programs.
    let allow_unsound = config.allow_syscalls;
    let wrapper = builder
        .create_wrapper_info(&executable_func, EntryCodeConfig::executable(allow_unsound))?;
    Ok((
        CompiledFunction {
            program: builder.casm_program().clone(),
            wrapper,
        },
        sierra_program,
        debug_info,
    ))
}
