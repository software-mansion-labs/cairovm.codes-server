// use crate::{
//     callstack::{get_callstack, CallstackEntry},
//     sierra_to_cairo::{get_sierra_to_cairo_debug_info, SierraToCairoDebugInfo},
//     sierra_vars::extract_sierra_vars_values,
// };
use cairo_lang_compiler::db::RootDatabase;
use cairo_lang_sierra_generator::program_generator::SierraProgramWithDebug;
use cairo_lang_sierra_to_casm::compiler::CairoProgramDebugInfo;
use cairo_vm::{
    Felt252,
    types::instruction::{Instruction, Op1Addr},
    utils::PRIME_STR,
    vm::{decoding::decoder::decode_instruction, trace::trace_entry::RelocatedTraceEntry},
};
use num_bigint::BigUint;
// use num_traits::cast::ToPrimitive;
use serde::{Serialize, Serializer};
use serde_json::json;
// use starknet_types_core::felt::Felt as Felt252;
use std::collections::HashMap;
use std::io::{Error, ErrorKind};

use super::{
    callstack::{CallstackEntry, get_callstack},
    sierra_to_cairo::{SierraToCairoDebugInfo, get_sierra_to_cairo_debug_info},
    sierra_vars::extract_sierra_vars_values,
};

#[derive(Debug)]
pub struct InstructionSerializable(Instruction);

impl Serialize for InstructionSerializable {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let instruction = &self.0;

        // Create a JSON object
        let j = json!({
            "off0": format!("{:?}", instruction.off0),
            "off1": format!("{:?}", instruction.off1),
            "off2": format!("{:?}", instruction.off2),
            "dst_register": format!("{:?}", instruction.dst_register),
            "op0_register": format!("{:?}", instruction.op0_register),
            "op1_addr": format!("{:?}", instruction.op1_addr),
            "res": format!("{:?}", instruction.res),
            "pc_update": format!("{:?}", instruction.pc_update),
            "ap_update": format!("{:?}", instruction.ap_update),
            "fp_update": format!("{:?}", instruction.fp_update),
            "opcode": format!("{:?}", instruction.opcode),
        });

        // Serialize the JSON object
        j.serialize(serializer)
    }
}

#[derive(Serialize, Debug)]
pub struct TracerData {
    pub pc_inst_map: HashMap<usize, InstructionSerializable>,
    pub trace: Vec<RelocatedTraceEntry>,
    pub memory: HashMap<usize, String>,
    pub pc_to_inst_indexes_map: HashMap<usize, usize>,
    pub callstack: Vec<Vec<CallstackEntry>>,
    pub trace_entries_to_sierra_vars: Vec<HashMap<u64, Vec<String>>>,
    pub sierra_to_cairo_debug_info: SierraToCairoDebugInfo,
}

pub fn make_trace_data(
    trace: Vec<RelocatedTraceEntry>,
    memory: Vec<Option<Felt252>>,
    casm_program_debug_info: &CairoProgramDebugInfo,
    casm_to_sierra_map: &HashMap<usize, Vec<usize>>,
    sierra_program_with_debug: &SierraProgramWithDebug,
    compiler_db: &RootDatabase,
) -> Result<TracerData, Error> {
    let sierra_to_cairo_debug_info =
        get_sierra_to_cairo_debug_info(sierra_program_with_debug, compiler_db);

    let mut pc_inst_map: HashMap<usize, Instruction> = HashMap::new();
    let mut pc_inst_serialized_map: HashMap<usize, InstructionSerializable> = HashMap::new();
    let mut pc_to_inst_indexes_map: HashMap<usize, usize> = HashMap::new();

    let max_pc_entry = trace.iter().max_by(|a, b| a.pc.cmp(&b.pc));

    let max_pc = match max_pc_entry {
        Some(max_entry) => max_entry.pc,
        None => {
            println!("No entries in the trace");
            0
        }
    };

    let mut skip_next_pc = false;
    let mut casm_index: usize = 0;
    for pc in 1..=max_pc {
        if skip_next_pc {
            skip_next_pc = false;
            continue;
        }

        let (instruction_encoding_felt, _) =
            get_instruction_encoding(pc, &memory).expect("Failed to get instruction encoding");

        let instruction_encoding_u64 = instruction_encoding_felt
            .try_into()
            .expect("Failed to convert felt to u64");
        let instruction =
            decode_instruction(instruction_encoding_u64).expect("Failed to decode instruction");
        pc_inst_map.insert(pc, instruction);
        if instruction.op1_addr == Op1Addr::Imm {
            skip_next_pc = true;
        }
        pc_inst_serialized_map.insert(pc, InstructionSerializable(instruction));
        pc_to_inst_indexes_map.insert(pc, casm_index);
        casm_index += 1;
    }

    let memory_map = memory
        .iter()
        .filter_map(|x| x.as_ref().map(|_| (*x).unwrap()))
        .map(|x| x.to_hex_string())
        .enumerate()
        .map(|(i, v)| (i + 1, v))
        .collect();

    let trace_entries_to_sierra_vars = extract_sierra_vars_values(
        &trace,
        &memory,
        &pc_to_inst_indexes_map,
        casm_to_sierra_map,
        casm_program_debug_info,
        &sierra_program_with_debug.program,
    );

    let callstack = get_callstack(
        &trace,
        &memory,
        &pc_inst_map,
        &pc_to_inst_indexes_map,
        casm_to_sierra_map,
        &sierra_to_cairo_debug_info,
        sierra_program_with_debug,
    );

    Ok(TracerData {
        pc_inst_map: pc_inst_serialized_map,
        trace,
        memory: memory_map,
        pc_to_inst_indexes_map,
        trace_entries_to_sierra_vars,
        callstack,
        sierra_to_cairo_debug_info,
    })
}

// Returns the encoded instruction (the value at pc) and the immediate value (the value at
// pc + 1, if it exists in the memory).
pub fn get_instruction_encoding(
    pc: usize,
    memory: &[Option<Felt252>],
) -> Result<(Felt252, Option<Felt252>), Error> {
    if memory[pc].is_none() {
        return Err(Error::new(ErrorKind::Other, ""));
    }
    let instruction_encoding = memory[pc].unwrap();
    let prime = BigUint::parse_bytes(PRIME_STR[2..].as_bytes(), 16).unwrap();

    let imm_addr = BigUint::from(pc + 1) % prime;
    let imm_addr =
        usize::try_from(imm_addr.clone()).map_err(|_| Error::new(ErrorKind::Other, ""))?;
    let optional_imm = memory[imm_addr];
    Ok((instruction_encoding, optional_imm))
}
