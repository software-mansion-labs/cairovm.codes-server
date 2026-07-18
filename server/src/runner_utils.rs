use crate::utils::capture_stdout;
use cairo_lang_runnable_utils::builder::{EntryCodeConfig, RunnableBuilder};
use cairo_lang_runner::{
    Arg, CairoHintProcessor, RunResult, RunResultStarknet, RunResultValue, RunnerError,
    SierraCasmRunner, StarknetState, build_hints_dict,
    casm_run::{self, RunFunctionResult},
    initialize_vm,
};
use cairo_lang_sierra::{
    extensions::{NamedType, enm::EnumType, gas::GasBuiltinType},
    ids::{ConcreteTypeId, GenericTypeId},
    program::{Function, GenericArg},
};
use cairo_lang_utils::{casts::IntoOrPanic, extract_matches};
use cairo_vm::{
    Felt252,
    hint_processor::hint_processor_definition::HintProcessor,
    serde::deserialize_program::HintParams,
    types::builtin_name::BuiltinName,
    vm::{runners::cairo_runner::RunResources, trace::trace_entry::RelocatedTraceEntry},
};
use num_bigint::BigInt;
use std::collections::HashMap;

/// Runs the vm starting from a function with custom hint processor. Function may have
/// implicits, but no other ref params. The cost of the function is deducted from
/// `available_gas` before the execution begins.
pub fn run_function<'a, Bytecode>(
    builder: &RunnableBuilder,
    func: &Function,
    hint_processor: &mut dyn HintProcessor,
    hints_dict: HashMap<usize, Vec<HintParams>>,
    bytecode: Bytecode,
    builtins: Vec<BuiltinName>,
) -> Result<(RunResult, Vec<RelocatedTraceEntry>, String), RunnerError>
where
    Bytecode: ExactSizeIterator<Item = &'a BigInt> + Clone,
{
    let return_types = builder.generic_id_and_size_from_concrete(&func.signature.ret_types);
    let data_len = bytecode.len();

    let (result, stdout) = capture_stdout(|| {
        casm_run::run_function(
            bytecode,
            builtins,
            |vm| initialize_vm(vm, data_len),
            hint_processor,
            hints_dict,
        )
    });
    let result = result?;

    let RunFunctionResult {
        ap,
        mut used_resources,
        memory,
        relocated_trace,
    } = result;

    let header_end = relocated_trace.last().unwrap().pc;
    used_resources.n_steps -= relocated_trace
        .iter()
        .position(|e| e.pc > header_end)
        .unwrap()
        - 1;
    used_resources.n_steps -= relocated_trace
        .iter()
        .rev()
        .position(|e| e.pc > header_end)
        .unwrap()
        - 1;

    let (results_data, gas_counter) = get_results_data(builder, &return_types, &memory, ap);
    assert!(results_data.len() <= 1);

    let value = if results_data.is_empty() {
        // No result type - no panic.
        RunResultValue::Success(vec![])
    } else {
        let (ty, values) = results_data[0].clone();
        let inner_ty =
            inner_type_from_panic_wrapper(builder, &ty, func).map(|it| builder.type_size(&it));
        SierraCasmRunner::handle_main_return_value(inner_ty, values, &memory)
    };

    // let profiling_info = self
    //     .run_profiler
    //     .as_ref()
    //     .map(|config| self.collect_profiling_info(&relocated_trace, config.clone()));

    let profiling_info = None;

    Ok((
        RunResult {
            gas_counter,
            memory,
            value,
            used_resources,
            profiling_info,
        },
        relocated_trace,
        stdout,
    ))
}

/// Returns the final values and type of all `func`s returning variables.
pub fn get_results_data(
    builder: &RunnableBuilder,
    return_types: &[(GenericTypeId, i16)],
    cells: &[Option<Felt252>],
    mut ap: usize,
) -> (Vec<(GenericTypeId, Vec<Felt252>)>, Option<Felt252>) {
    let mut results_data = vec![];
    for (ty, ty_size) in return_types.iter().rev() {
        let size = *ty_size as usize;
        let values: Vec<Felt252> = ((ap - size)..ap)
            .map(|index| cells[index].unwrap())
            .collect();
        ap -= size;
        results_data.push((ty.clone(), values));
    }

    // Handling implicits.
    let mut gas_counter = None;
    results_data.retain_mut(|(ty, values)| {
        let generic_ty = ty;
        if *generic_ty == GasBuiltinType::ID {
            gas_counter = Some(values.remove(0));
            assert!(values.is_empty());
            false
        } else {
            builder.is_user_arg_type(generic_ty)
        }
    });

    (results_data, gas_counter)
}

/// Extract inner type if `ty` is a panic wrapper
pub fn inner_type_from_panic_wrapper(
    builder: &RunnableBuilder,
    ty: &GenericTypeId,
    func: &Function,
) -> Option<ConcreteTypeId> {
    let generic_args = &func
        .signature
        .ret_types
        .iter()
        .find_map(|rt| {
            let long_id = builder.type_long_id(rt);
            (long_id.generic_id == *ty).then_some(long_id)
        })
        .unwrap()
        .generic_args;

    if *ty == EnumType::ID
        && matches!(&generic_args[0], GenericArg::UserType(ut)
            if ut.debug_name.as_ref().unwrap().starts_with("core::panics::PanicResult::"))
    {
        return Some(extract_matches!(&generic_args[1], GenericArg::Type).clone());
    }
    None
}

/// Runs the vm starting from a function in the context of a given starknet state.
pub fn run_function_with_starknet_context(
    runner: &SierraCasmRunner,
    builder: &RunnableBuilder,
    func: &Function,
    args: Vec<Arg>,
    available_gas: Option<usize>,
    starknet_state: StarknetState,
) -> Result<(RunResultStarknet, Vec<RelocatedTraceEntry>, String), RunnerError> {
    let (assembled_program, builtins) =
        builder.assemble_function_program(func, EntryCodeConfig::testing())?;
    let (hints_dict, string_to_hint) = build_hints_dict(&assembled_program.hints);
    let user_args = prepare_args(builder, func, available_gas, args)?;
    let mut hint_processor = CairoHintProcessor {
        runner: Some(runner),
        user_args,
        starknet_state,
        string_to_hint,
        run_resources: RunResources::default(),
        syscalls_used_resources: Default::default(),
        no_temporary_segments: true,
        markers: Default::default(),
    };
    let (
        RunResult {
            gas_counter,
            memory,
            value,
            used_resources,
            profiling_info,
        },
        relocated_trace,
        stdout,
    ) = run_function(
        builder,
        func,
        &mut hint_processor,
        hints_dict,
        assembled_program.bytecode.iter(),
        builtins,
    )?;
    let mut all_used_resources = hint_processor.syscalls_used_resources;
    all_used_resources.basic_resources += &used_resources;
    Ok((
        RunResultStarknet {
            gas_counter,
            memory,
            value,
            starknet_state: hint_processor.starknet_state,
            used_resources: all_used_resources,
            profiling_info,
        },
        relocated_trace,
        stdout,
    ))
}

/// Groups the args by parameters, and additionally add `gas` as the first if required.
fn prepare_args(
    builder: &RunnableBuilder,
    func: &Function,
    _available_gas: Option<usize>,
    args: Vec<Arg>,
) -> Result<Vec<Vec<Arg>>, RunnerError> {
    let mut user_args = vec![];
    // if let Some(gas) = requires_gas_builtin(builder, func).then_some(get_initial_available_gas(
    //     builder,
    //     func,
    //     available_gas,
    // )?) {
    //     user_args.push(vec![Arg::Value(Felt252::from(gas))]);
    // }
    if let Some(gas) = requires_gas_builtin(builder, func).then_some(usize::MAX) {
        user_args.push(vec![Arg::Value(Felt252::from(gas))]);
    }
    let mut expected_arguments_size = 0;
    let actual_args_size = args_size(&args);
    let mut arg_iter = args.into_iter().enumerate();
    for (param_index, (_, param_size)) in builder
        .generic_id_and_size_from_concrete(&func.signature.param_types)
        .into_iter()
        .filter(|(ty, _)| builder.is_user_arg_type(ty))
        .enumerate()
    {
        let mut curr_arg = vec![];
        let param_size: usize = param_size.into_or_panic();
        expected_arguments_size += param_size;
        let mut taken_size = 0;
        while taken_size < param_size {
            let Some((arg_index, arg)) = arg_iter.next() else {
                break;
            };
            taken_size += arg.size();
            if taken_size > param_size {
                return Err(RunnerError::ArgumentUnaligned {
                    param_index,
                    arg_index,
                });
            }
            curr_arg.push(arg);
        }
        user_args.push(curr_arg);
    }
    if expected_arguments_size != actual_args_size {
        return Err(RunnerError::ArgumentsSizeMismatch {
            expected: expected_arguments_size,
            actual: actual_args_size,
        });
    }
    Ok(user_args)
}

/// Returns whether the gas builtin is required in the given function.
fn requires_gas_builtin(builder: &RunnableBuilder, func: &Function) -> bool {
    func.signature
        .param_types
        .iter()
        .any(|ty| builder.type_long_id(ty).generic_id == GasBuiltinType::ID)
}

// Returns the initial value for the gas counter.
// If `available_gas` is None returns 0.
// pub fn get_initial_available_gas(
//     builder: &RunnableBuilder,
//     func: &Function,
//     available_gas: Option<usize>,
// ) -> Result<usize, RunnerError> {
//     println!("get_initial_available_gas");
//     let Some(available_gas) = available_gas else {
//         println!("get_initial_available_gas return Ok(0);");
//         return Ok(0);
//     };

//     // In case we don't have any costs - it means no gas equations were solved (and we are in
//     // the case of no gas checking enabled) - so the gas builtin is irrelevant, and we
//     // can return any value.
//     let Some(required_gas) = initial_required_gas(builder, func) else {
//         println!("get_initial_available_gas return Ok(0);");
//         return Ok(0);
//     };

//     let res = available_gas
//         .checked_sub(required_gas)
//         .ok_or(RunnerError::NotEnoughGasToCall);
//     println!("res {:?}", res);
//     res
// }

// fn initial_required_gas(builder: &RunnableBuilder, func: &Function) -> Option<usize> {
//     let gas_info = &builder.metadata().gas_info;
//     require(!gas_info.function_costs.is_empty())?;
//     Some(
//         gas_info.function_costs[&func.id]
//             .iter()
//             .map(|(token_type, val)| val.into_or_panic::<usize>() * token_gas_cost(*token_type))
//             .sum(),
//     )
// }

/// The size in memory of the arguments.
fn args_size(args: &[Arg]) -> usize {
    args.iter().map(Arg::size).sum()
}
