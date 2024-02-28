use byteorder::{ByteOrder, LittleEndian};
use cairo_vm::{
    types::instruction::{Instruction, Opcode},
    vm::trace::trace_entry::RelocatedTraceEntry,
    Felt252,
};
use serde::Serialize;
use std::collections::HashMap;

const MAX_TRACEBACK_ENTRIES: usize = 100;

#[derive(Serialize, Debug)]
pub struct CallstackEntry {
    pub fp: usize,
    pub call_pc: Option<usize>,
    pub ret_pc: Option<usize>,
    pub fn_name: Option<String>,
}

/// Returs callstack for each trace entry in the trace.
pub fn get_callstack(
    trace: &Vec<RelocatedTraceEntry>,
    memory: &Vec<Option<Felt252>>,
    pc_inst_map: &HashMap<usize, Instruction>,
    pc_to_inst_indexes_map: &HashMap<usize, usize>,
    casm_to_sierra_map: &HashMap<usize, Vec<usize>>,
    sierra_to_cairo_fn_names_map: &HashMap<usize, String>,
) -> Vec<Vec<CallstackEntry>> {
    let mut callstack: Vec<Vec<CallstackEntry>> = Vec::new();

    let mut fp_to_fn_name: HashMap<usize, String> = HashMap::new();

    for (trace_entry_index, trace_entry) in trace.iter().enumerate() {
        callstack.push(Vec::new());

        let mut fp = trace_entry.fp;

        let fn_name: Option<String> = get_fn_name_at_pc(
            &trace_entry.pc,
            &fp,
            &pc_to_inst_indexes_map,
            &casm_to_sierra_map,
            &sierra_to_cairo_fn_names_map,
            &mut fp_to_fn_name,
        );

        let callstack_entry = CallstackEntry {
            fp,
            call_pc: None,
            ret_pc: None,
            fn_name,
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
                    &sierra_to_cairo_fn_names_map,
                    &mut fp_to_fn_name,
                );

                let callstack_entry = CallstackEntry {
                    fp,
                    call_pc: Some(call_pc),
                    ret_pc: Some(ret_pc),
                    fn_name,
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
    sierra_to_cairo_fn_names_map: &HashMap<usize, String>,
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
                if let Some(name) = sierra_to_cairo_fn_names_map.get(sierra_index) {
                    fp_to_fn_name.insert(fp.clone(), name.clone());
                    return Some(name.clone());
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
