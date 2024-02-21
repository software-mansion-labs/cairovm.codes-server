use crate::sierra_vars::{extract_sierra_vars_values, SierraVariablesTraceDebugInfo};

use byteorder::{ByteOrder, LittleEndian};
use cairo_lang_sierra::program::Program;
use cairo_lang_sierra_to_casm::compiler::CairoProgramDebugInfo;
use cairo_vm::{
    types::instruction::{Instruction, Op1Addr},
    utils::PRIME_STR,
    vm::{decoding::decoder::decode_instruction, trace::trace_entry::RelocatedTraceEntry},
    Felt252,
};
use num_bigint::BigUint;
use serde::{Serialize, Serializer};
use serde_json::json;
use std::collections::HashMap;
use std::io::{Error, ErrorKind};

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

#[derive(Serialize)]
pub struct TracerData {
    pub pc_inst_map: HashMap<usize, InstructionSerializable>,
    pub trace: Vec<RelocatedTraceEntry>,
    pub memory: HashMap<usize, String>,
    pub pc_to_inst_indexes_map: HashMap<usize, usize>,
    pub trace_entries_to_sierra_vars: Vec<SierraVariablesTraceDebugInfo>,
}

pub fn make_trace_data(
    trace: Vec<RelocatedTraceEntry>,
    memory: Vec<Option<Felt252>>,
    casm_program_debug_info: &CairoProgramDebugInfo,
    casm_to_sierra_map: &HashMap<usize, Vec<usize>>,
    sierra_program: &Program,
) -> TracerData {
    let mut pc_inst_map: HashMap<usize, InstructionSerializable> = HashMap::new();
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
        let instruction_encoding_bytes_le = instruction_encoding_felt.to_bytes_le();
        let instruction_encoding_u64 = LittleEndian::read_u64(&instruction_encoding_bytes_le[..]);
        let instruction =
            decode_instruction(instruction_encoding_u64).expect("Failed to decode instruction");
        if instruction.op1_addr == Op1Addr::Imm {
            skip_next_pc = true;
        }
        pc_inst_map.insert(pc, InstructionSerializable(instruction));
        pc_to_inst_indexes_map.insert(pc, casm_index);
        casm_index += 1;
    }

    let memory_map = memory
        .iter()
        .filter_map(|x| x.as_ref().map(|_| x.clone().unwrap()))
        .map(|x| x.to_hex_string())
        .enumerate()
        .map(|(i, v)| (i + 1, v))
        .collect();

    let trace_entries_to_sierra_vars: Vec<SierraVariablesTraceDebugInfo> =
        extract_sierra_vars_values(
            &trace,
            &memory,
            &pc_to_inst_indexes_map,
            &casm_to_sierra_map,
            &casm_program_debug_info,
            &sierra_program,
        );

    TracerData {
        pc_inst_map,
        trace,
        memory: memory_map,
        pc_to_inst_indexes_map,
        trace_entries_to_sierra_vars,
    }
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
    let instruction_encoding = memory[pc].clone().unwrap();
    let prime = BigUint::parse_bytes(PRIME_STR[2..].as_bytes(), 16).unwrap();

    let imm_addr = BigUint::from(pc + 1) % prime;
    let imm_addr =
        usize::try_from(imm_addr.clone()).map_err(|_| Error::new(ErrorKind::Other, ""))?;
    let optional_imm = memory[imm_addr].clone();
    Ok((instruction_encoding, optional_imm))
}
