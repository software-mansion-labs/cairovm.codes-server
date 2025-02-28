use cairo_lang_casm::{
    ap_change::ApChange,
    cell_expression::{CellExpression, CellOperator},
    operand::{CellRef, DerefOrImmediate, Register},
};
use cairo_lang_sierra::program::{GenStatement, Program};
use cairo_lang_sierra_to_casm::compiler::{CairoProgramDebugInfo, StatementKindDebugInfo};
use cairo_vm::{vm::trace::trace_entry::RelocatedTraceEntry, Felt252};
use num_traits::cast::ToPrimitive;
// use starknet_types_core::felt::Felt as Felt252;
use std::collections::HashMap;

pub fn extract_sierra_vars_values(
    trace: &Vec<RelocatedTraceEntry>,
    memory: &Vec<Option<Felt252>>,
    pc_to_inst_indexes_map: &HashMap<usize, usize>,
    casm_to_sierra_map: &HashMap<usize, Vec<usize>>,
    casm_program_debug_info: &CairoProgramDebugInfo,
    sierra_program: &Program,
) -> Vec<HashMap<u64, Vec<String>>> {
    let mut trace_entries_to_sierra_vars: Vec<HashMap<u64, Vec<String>>> = Vec::new();

    for (trace_entry_index, trace_entry) in trace.iter().enumerate() {
        trace_entries_to_sierra_vars.push(HashMap::new());

        let casm_inst_index = pc_to_inst_indexes_map.get(&trace_entry.pc);
        if let Some(casm_inst_index) = casm_inst_index {
            let sierra_statements_indexes = casm_to_sierra_map.get(&casm_inst_index);
            if let Some(sierra_statements_indexes) = sierra_statements_indexes {
                for sierra_statement_index in sierra_statements_indexes {
                    let sierra_statement_debug_info = casm_program_debug_info
                        .sierra_statement_info
                        .get(*sierra_statement_index);
                    let sierra_statement = sierra_program.statements.get(*sierra_statement_index);

                    if let (Some(sierra_statement), Some(sierra_statement_debug_info)) =
                        (sierra_statement, sierra_statement_debug_info)
                    {
                        match (
                            &sierra_statement,
                            &sierra_statement_debug_info.additional_kind_info,
                        ) {
                            (
                                GenStatement::Invocation(invocation),
                                StatementKindDebugInfo::Invoke(additional_kind_info),
                            ) => {
                                for (branch_index, branch_change) in additional_kind_info
                                    .result_branch_changes
                                    .iter()
                                    .enumerate()
                                {
                                    let branch_info = &invocation.branches[branch_index];
                                    for (output_reference_index, output_reference_value) in
                                        branch_change.refs.iter().enumerate()
                                    {
                                        let values = get_values_from_cell_expressions(
                                            &memory,
                                            &trace_entry,
                                            &output_reference_value.expression.cells,
                                            &branch_change.ap_change,
                                        );
                                        trace_entries_to_sierra_vars[trace_entry_index].insert(
                                            branch_info.results[output_reference_index].id,
                                            values,
                                        );
                                    }
                                }

                                for (invoke_ref_index, invoke_ref) in
                                    additional_kind_info.ref_values.iter().enumerate()
                                {
                                    let values = get_values_from_cell_expressions(
                                        &memory,
                                        &trace_entry,
                                        &invoke_ref.expression.cells,
                                        &ApChange::Known(0),
                                    );
                                    trace_entries_to_sierra_vars[trace_entry_index]
                                        .insert(invocation.args[invoke_ref_index].id, values);
                                }
                            }
                            (
                                GenStatement::Return(return_vars),
                                StatementKindDebugInfo::Return(additional_kind_info),
                            ) => {
                                for (return_ref_index, return_ref) in
                                    additional_kind_info.ref_values.iter().enumerate()
                                {
                                    let values = get_values_from_cell_expressions(
                                        &memory,
                                        &trace_entry,
                                        &return_ref.expression.cells,
                                        &ApChange::Known(0),
                                    );
                                    trace_entries_to_sierra_vars[trace_entry_index]
                                        .insert(return_vars[return_ref_index].id, values);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    trace_entries_to_sierra_vars
}

pub fn get_values_from_cell_expressions(
    memory: &Vec<Option<Felt252>>,
    trace_entry: &RelocatedTraceEntry,
    cell_expressions: &Vec<CellExpression>,
    ap_change: &ApChange,
) -> Vec<String> {
    let mut value_vec: Vec<String> = Vec::new();
    for cell_expression in cell_expressions {
        let value =
            get_value_from_cell_expression(&memory, &trace_entry, &cell_expression, &ap_change);
        match value {
            Ok(value) => {
                value_vec.push(value);
            }
            Err(e) => match e {
                GetCellRefValueError::UnknownApChange => {}
                _ => {
                    // dbg!(e);
                }
            },
        }
    }
    value_vec
}

#[derive(Debug)]
pub enum GetCellRefValueError {
    UnknownApChange,
    MemoryAddressNotFound,
    OtherError(String),
}

pub fn get_cell_ref_value(
    memory: &Vec<Option<Felt252>>,
    trace_entry: &RelocatedTraceEntry,
    cell_ref: &CellRef,
    ap_change: &ApChange,
) -> Result<Felt252, GetCellRefValueError> {
    match cell_ref.register {
        Register::AP => match ap_change {
            ApChange::Known(ap_change_value) => {
                let ap: i32 = trace_entry.ap as i32;
                let addr = ap + cell_ref.offset as i32 + *ap_change_value as i32;
                memory[addr as usize].ok_or(GetCellRefValueError::MemoryAddressNotFound)
            }
            ApChange::Unknown => Err(GetCellRefValueError::UnknownApChange),
        },
        Register::FP => {
            let fp: i32 = trace_entry.fp as i32;
            let addr = fp + cell_ref.offset as i32;
            memory[addr as usize].ok_or(GetCellRefValueError::MemoryAddressNotFound)
        }
    }
}

pub fn get_value_from_cell_expression(
    memory: &Vec<Option<Felt252>>,
    trace_entry: &RelocatedTraceEntry,
    cell_expression: &CellExpression,
    ap_change: &ApChange,
) -> Result<String, GetCellRefValueError> {
    match cell_expression {
        CellExpression::Deref(cell_ref) => {
            get_cell_ref_value(memory, trace_entry, cell_ref, ap_change)
                .map(|value| value.to_hex_string())
        }
        CellExpression::Immediate(imm) => Ok(format!("0x{:x}", imm)),
        CellExpression::DoubleDeref(cell_ref, offset) => {
            match get_cell_ref_value(memory, trace_entry, cell_ref, ap_change) {
                Ok(cell_ref_value_felt) => {
                    let cell_ref_value: Option<i128> = cell_ref_value_felt.to_i128();
                    if let Some(cell_ref_value) = cell_ref_value {
                        let addr = cell_ref_value + *offset as i128;
                        let value = memory.get(addr as usize).cloned();
                        if let Some(Some(value)) = value {
                            Ok(value.to_string())
                        } else {
                            Err(GetCellRefValueError::MemoryAddressNotFound)
                        }
                    } else {
                        Err(GetCellRefValueError::MemoryAddressNotFound)
                    }
                }
                Err(e) => Err(e),
            }
        }
        CellExpression::BinOp { op, a, b } => {
            let a = get_cell_ref_value(memory, trace_entry, a, ap_change);
            match a {
                Ok(a) => {
                    let b = match b {
                        DerefOrImmediate::Deref(cell) => {
                            get_cell_ref_value(memory, trace_entry, cell, ap_change)
                        }
                        DerefOrImmediate::Immediate(x) => Ok(Felt252::from(&x.value)),
                    };

                    match b {
                        Ok(b) => {
                            let value: Result<Felt252, GetCellRefValueError> = match op {
                                CellOperator::Add => Ok(a + b),
                                CellOperator::Mul => Ok(a * b),
                                CellOperator::Div => match b.try_into() {
                                    Ok(b) => Ok(a.field_div(&b)),
                                    Err(_) => Err(GetCellRefValueError::OtherError(
                                        "Division by zero".to_string(),
                                    )),
                                },
                                CellOperator::Sub => Ok(a - b),
                            };
                            match value {
                                Ok(value) => Ok(value.to_hex_string()),
                                Err(e) => Err(e),
                            }
                        }
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(e),
            }
        }
    }
}
