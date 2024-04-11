use crate::sierra_to_cairo::SierraToCairoDebugInfo;
use byteorder::{ByteOrder, LittleEndian};
use cairo_lang_sierra::{
    extensions::core::{CoreLibfunc, CoreType},
    program::Function,
    program_registry::ProgramRegistry,
};
use cairo_lang_sierra_generator::program_generator::SierraProgramWithDebug;
use cairo_lang_sierra_type_size::{get_type_size_map, TypeSizeMap};
use cairo_vm::{
    types::instruction::{Instruction, Opcode},
    vm::trace::trace_entry::RelocatedTraceEntry,
    Felt252,
};
use serde::Serialize;
use std::collections::HashMap;

const MAX_TRACEBACK_ENTRIES: usize = 100;

#[derive(Serialize, Debug)]
pub struct Param {
    pub type_name: Option<String>,
    pub value: usize,
}

#[derive(Serialize, Debug)]
pub struct CallstackEntry {
    pub fp: usize,
    pub call_pc: Option<usize>,
    pub ret_pc: Option<usize>,
    pub fn_name: Option<String>,
    pub params: Vec<Param>,
}

/// Returs callstack for each trace entry in the trace.
pub fn get_callstack(
    trace: &Vec<RelocatedTraceEntry>,
    memory: &Vec<Option<Felt252>>,
    pc_inst_map: &HashMap<usize, Instruction>,
    pc_to_inst_indexes_map: &HashMap<usize, usize>,
    casm_to_sierra_map: &HashMap<usize, Vec<usize>>,
    sierra_to_cairo_debug_info: &SierraToCairoDebugInfo,
    sierra_program_with_debug: &SierraProgramWithDebug,
) -> Vec<Vec<CallstackEntry>> {
    let mut callstack: Vec<Vec<CallstackEntry>> = Vec::new();

    let mut fp_to_fn_name: HashMap<usize, String> = HashMap::new();

    let sierra_program_registry: ProgramRegistry<CoreType, CoreLibfunc> =
        ProgramRegistry::<CoreType, CoreLibfunc>::new(&sierra_program_with_debug.program)
            .expect("Failed to create program registry");
    let type_sizes =
        get_type_size_map(&sierra_program_with_debug.program, &sierra_program_registry)
            .unwrap_or_default();

    for (trace_entry_index, trace_entry) in trace.iter().enumerate() {
        callstack.push(Vec::new());

        let mut fp = trace_entry.fp;

        let fn_name: Option<String> = get_fn_name_at_pc(
            &trace_entry.pc,
            &fp,
            &pc_to_inst_indexes_map,
            &casm_to_sierra_map,
            &sierra_to_cairo_debug_info,
            &mut fp_to_fn_name,
        );

        let params = match &fn_name {
            Some(fn_name) => {
                get_params(fn_name, sierra_program_with_debug, &type_sizes, memory, fp)
            }
            None => Vec::new(),
        };

        let callstack_entry = CallstackEntry {
            fp,
            call_pc: None,
            ret_pc: None,
            fn_name,
            params,
        };

        callstack[trace_entry_index].push(callstack_entry);

        for _ in 0..MAX_TRACEBACK_ENTRIES {
            let opt_fp = get_memory_usize_value_at_index(&memory, fp - 2);
            let opt_ret_pc = get_memory_usize_value_at_index(&memory, fp - 1);
            if let Some(opt_fp) = opt_fp {
                if opt_fp == fp {
                    break;
                }
            }

            if let (Some(opt_fp), Some(opt_ret_pc)) = (opt_fp, opt_ret_pc) {
                fp = opt_fp;
                let ret_pc = opt_ret_pc;

                let instruction0_value = memory.get(ret_pc - 2).cloned().flatten();
                let instruction0 = pc_inst_map.get(&(ret_pc - 2));
                let instruction1_value = memory.get(ret_pc - 1).cloned().flatten();
                let instruction1 = pc_inst_map.get(&(ret_pc - 1));

                let call_pc;

                match (
                    instruction0_value,
                    instruction0,
                    instruction1_value,
                    instruction1,
                ) {
                    (_, _, Some(_instruction1_value), Some(instruction1))
                        if instruction1.opcode == Opcode::Call =>
                    {
                        call_pc = ret_pc - 1;
                    }
                    (
                        Some(_instruction0_value),
                        Some(instruction0),
                        Some(_instruction1_value),
                        _,
                    ) if instruction0.opcode == Opcode::Call => {
                        call_pc = ret_pc - 2;
                    }
                    _ => {
                        break;
                    }
                }

                let fn_name: Option<String> = get_fn_name_at_pc(
                    &call_pc,
                    &fp,
                    &pc_to_inst_indexes_map,
                    &casm_to_sierra_map,
                    &sierra_to_cairo_debug_info,
                    &mut fp_to_fn_name,
                );

                let params = match &fn_name {
                    Some(fn_name) => {
                        get_params(fn_name, sierra_program_with_debug, &type_sizes, memory, fp)
                    }
                    None => Vec::new(),
                };

                let callstack_entry = CallstackEntry {
                    fp,
                    call_pc: Some(call_pc),
                    ret_pc: Some(ret_pc),
                    fn_name,
                    params,
                };

                callstack[trace_entry_index].push(callstack_entry);
            } else {
                break;
            }
        }
    }

    for callstack_entry in callstack.iter_mut() {
        for entry in callstack_entry.iter_mut() {
            if entry.fn_name.is_none() {
                entry.fn_name = fp_to_fn_name.get(&entry.fp).cloned();
            }
        }
    }

    callstack
}

/// Returns the function name at the given pc.
pub fn get_fn_name_at_pc(
    pc: &usize,
    fp: &usize,
    pc_to_inst_indexes_map: &HashMap<usize, usize>,
    casm_to_sierra_map: &HashMap<usize, Vec<usize>>,
    sierra_to_cairo_debug_info: &SierraToCairoDebugInfo,
    fp_to_fn_name: &mut HashMap<usize, String>,
) -> Option<String> {
    if let Some(fn_name) = fp_to_fn_name.get(fp) {
        return Some(fn_name.clone());
    }
    let inst_index = pc_to_inst_indexes_map.get(&pc);
    if let Some(index) = inst_index {
        let sierra_indexes = casm_to_sierra_map.get(index);
        if let Some(sierra_indexes) = sierra_indexes {
            for sierra_index in sierra_indexes {
                if let Some(sierra_statement_to_cairo_debug_info) = sierra_to_cairo_debug_info
                    .sierra_statements_to_cairo_info
                    .get(sierra_index)
                {
                    if let Some(fn_name) = &sierra_statement_to_cairo_debug_info.fn_name {
                        fp_to_fn_name.insert(fp.clone(), fn_name.clone());
                        return Some(fn_name.clone());
                    }
                }
            }
        }
    }
    None
}

/// Returns the usize value at the given index in the memory.
pub fn get_memory_usize_value_at_index(
    memory: &Vec<Option<Felt252>>,
    index: usize,
) -> Option<usize> {
    match memory.get(index) {
        Some(Some(value_felt)) => {
            let value_bytes_le = value_felt.to_bytes_le();
            let value = LittleEndian::read_u128(&value_bytes_le[..]) as usize;
            Some(value)
        }
        _ => None,
    }
}

fn find_function<'a>(
    sierra_program: &'a SierraProgramWithDebug,
    function_name: &str,
) -> Option<&'a Function> {
    sierra_program.program.funcs.iter().find(|f| {
        if let Some(name) = &f.id.debug_name {
            name == function_name
        } else {
            false
        }
    })
}

fn get_params(
    fn_name: &str,
    sierra_program_with_debug: &SierraProgramWithDebug,
    type_sizes: &TypeSizeMap,
    memory: &Vec<Option<Felt252>>,
    fp: usize,
) -> Vec<Param> {
    let mut params: Vec<Param> = Vec::new();
    let function = find_function(sierra_program_with_debug, &fn_name);
    if let Some(function) = function {
        let mut memory_offset = 0;
        for param_type in function.signature.param_types.iter().rev() {
            if let Some(size) = type_sizes.get(&param_type) {
                memory_offset += size.clone() as usize;
                let value = get_memory_usize_value_at_index(&memory, fp - 2 - memory_offset);
                if let Some(value) = value {
                    params.push(Param {
                        type_name: param_type.debug_name.clone().map(|s| s.to_string()),
                        value,
                    });
                } else {
                    println!("Failed to get value for type {:?}", param_type);
                    return Vec::new();
                }
            } else {
                println!("Failed to get size for type {:?}", param_type);
                return Vec::new();
            }
        }
    }
    params.reverse();
    params
}
