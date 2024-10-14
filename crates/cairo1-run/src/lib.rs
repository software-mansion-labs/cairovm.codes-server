/*
 * Source: https://github.com/lambdaclass/cairo-vm/tree/main/cairo1-run
 * Modified to be used as a library.
 */

#![allow(unused_imports)]
use bincode::enc::write::Writer;
use cairo_lang_casm::casm;
use cairo_lang_casm::casm_extend;
use cairo_lang_casm::hints::Hint;
use cairo_lang_casm::instructions::Instruction;
use cairo_lang_compiler::db;
use cairo_lang_compiler::db::RootDatabase;
use cairo_lang_compiler::diagnostics::DiagnosticsReporter;
use cairo_lang_compiler::project::setup_project;
use cairo_lang_compiler::{compile_prepared_db, CompilerConfig};
use cairo_lang_diagnostics::FormattedDiagnosticEntry;
use cairo_lang_filesystem::cfg::{Cfg, CfgSet};
use cairo_lang_runner::casm_run::{CairoHintProcessor, RunFunctionContext};
use cairo_lang_runner::{build_hints_dict, Arg, SierraCasmRunner, StarknetState};
use cairo_lang_sierra::extensions::bitwise::BitwiseType;
use cairo_lang_sierra::extensions::core::{CoreLibfunc, CoreType};
use cairo_lang_sierra::extensions::ec::EcOpType;
use cairo_lang_sierra::extensions::gas::GasBuiltinType;
use cairo_lang_sierra::extensions::pedersen::PedersenType;
use cairo_lang_sierra::extensions::poseidon::PoseidonType;
use cairo_lang_sierra::extensions::range_check::RangeCheckType;
use cairo_lang_sierra::extensions::segment_arena::SegmentArenaType;
use cairo_lang_sierra::extensions::starknet::syscalls::SystemType;
use cairo_lang_sierra::extensions::ConcreteType;
use cairo_lang_sierra::extensions::NamedType;
use cairo_lang_sierra::ids::ConcreteTypeId;
use cairo_lang_sierra::program;
use cairo_lang_sierra::program::Function;
use cairo_lang_sierra::program::Program as SierraProgram;
use cairo_lang_sierra::program::Statement;
use cairo_lang_sierra::program::StatementIdx;
use cairo_lang_sierra::program_registry::{ProgramRegistry, ProgramRegistryError};
use cairo_lang_sierra::{extensions::gas::CostTokenType, ProgramParser};
use cairo_lang_sierra_ap_change::calc_ap_changes;
use cairo_lang_sierra_gas::compute_costs::CostTypeTrait;
use cairo_lang_sierra_gas::core_libfunc_cost;
use cairo_lang_sierra_gas::core_libfunc_cost::core_libfunc_cost as libfunc_cost;
use cairo_lang_sierra_gas::core_libfunc_cost::InvocationCostInfoProvider;
use cairo_lang_sierra_gas::gas_info::GasInfo;
use cairo_lang_sierra_gas::objects::ConstCost;
use cairo_lang_sierra_gas::objects::PreCost;
use cairo_lang_sierra_generator::program_generator::SierraProgramWithDebug;
use cairo_lang_sierra_generator::replace_ids::DebugReplacer;
use cairo_lang_sierra_to_casm::compiler::CairoProgram;
use cairo_lang_sierra_to_casm::compiler::CompilationError;
use cairo_lang_sierra_to_casm::compiler::SierraToCasmConfig;
use cairo_lang_sierra_to_casm::metadata::Metadata;
use cairo_lang_sierra_to_casm::metadata::MetadataComputationConfig;
use cairo_lang_sierra_to_casm::metadata::MetadataError;
use cairo_lang_sierra_to_casm::{compiler::compile, metadata::calc_metadata};
use cairo_lang_sierra_type_size::get_type_size_map;
use cairo_lang_sierra_type_size::TypeSizeMap;
use cairo_lang_starknet::contract::get_contracts_info;
use cairo_lang_starknet::starknet_plugin_suite;
use cairo_lang_test_plugin::test_plugin_suite;
use cairo_lang_utils::casts::IntoOrPanic;
use cairo_lang_utils::extract_matches;
use cairo_lang_utils::ordered_hash_map::OrderedHashMap;
use cairo_lang_utils::unordered_hash_map::UnorderedHashMap;
use cairo_vm::air_public_input::PublicInputError;
use cairo_vm::cairo_run;
use cairo_vm::cairo_run::EncodeTraceError;
use cairo_vm::hint_processor::cairo_1_hint_processor::hint_processor::Cairo1HintProcessor;
use cairo_vm::serde::deserialize_program::{ApTracking, FlowTrackingData, HintParams};
use cairo_vm::types::builtin_name::BuiltinName;
use cairo_vm::types::errors::program_errors::ProgramError;
use cairo_vm::types::layout_name::LayoutName;
use cairo_vm::types::relocatable::Relocatable;
use cairo_vm::vm::decoding::decoder::decode_instruction;
use cairo_vm::vm::errors::cairo_run_errors::CairoRunError;
use cairo_vm::vm::errors::memory_errors::MemoryError;
use cairo_vm::vm::errors::runner_errors::RunnerError;
use cairo_vm::vm::errors::trace_errors::TraceError;
use cairo_vm::vm::errors::vm_errors::VirtualMachineError;
use cairo_vm::vm::runners::cairo_runner::CairoArg;
use cairo_vm::vm::runners::cairo_runner::RunnerMode;
use cairo_vm::vm::trace::trace_entry::RelocatedTraceEntry;
use cairo_vm::vm::vm_memory::memory::Memory;
use cairo_vm::{
    serde::deserialize_program::ReferenceManager,
    types::{program::Program, relocatable::MaybeRelocatable},
    vm::{
        runners::cairo_runner::{CairoRunner, RunResources},
        vm_core::VirtualMachine,
    },
};
use serde::Serialize;
use starknet_types_core::felt::Felt as Felt252;
// use clap::{CommandFactory, Parser, ValueHint};
use core::panic;
use itertools::{chain, Itertools};
use std::borrow::Cow;
use std::io::BufWriter;
use std::io::Write;
use std::iter::Peekable;
use std::path::PathBuf;
use std::slice::Iter;
use std::{collections::HashMap, io, path::Path};
use thiserror::Error;

pub const CAIRO_LANG_COMPILER_VERSION: &str = "2.8.0";

// #[derive(Parser, Debug)]
// #[clap(author, version, about, long_about = None)]
// struct Args {
//     #[clap(value_parser, value_hint=ValueHint::FilePath)]
//     filename: PathBuf,
//     #[clap(long = "trace_file", value_parser)]
//     trace_file: Option<PathBuf>,
//     #[structopt(long = "memory_file")]
//     memory_file: Option<PathBuf>,
//     #[clap(long = "layout", default_value = "plain", value_parser=validate_layout)]
//     layout: String,
//     #[clap(long = "proof_mode", value_parser)]
//     proof_mode: bool,
//     #[clap(long = "air_public_input", requires = "proof_mode")]
//     air_public_input: Option<PathBuf>,
//     #[clap(
//         long = "air_private_input",
//         requires_all = ["proof_mode", "trace_file", "memory_file"]
//     )]
//     air_private_input: Option<PathBuf>,
//     #[clap(
//         long = "cairo_pie_output",
//         // We need to add these air_private_input & air_public_input or else
//         // passing cairo_pie_output + either of these without proof_mode will not fail
//         conflicts_with_all = ["proof_mode", "air_private_input", "air_public_input"]
//     )]
//     cairo_pie_output: Option<PathBuf>,
//     // Arguments should be spaced, with array elements placed between brackets
//     // For example " --args '1 2 [1 2 3]'" will yield 3 arguments, with the last one being an array of 3 elements
//     #[clap(long = "args", default_value = "", value_parser=process_args)]
//     args: FuncArgs,
//     #[clap(long = "print_output", value_parser)]
//     print_output: bool,
// }

fn process_args(value: &str) -> Result<Vec<Arg>, String> {
    if value.is_empty() {
        return Ok(Vec::new());
    }

    let mut args = Vec::new();
    let mut input = value.split(' ');

    while let Some(value) = input.next() {
        // First argument in an array
        if value.starts_with('[') {
            let mut array_arg = vec![Arg::from(
                Felt252::from_dec_str(value.strip_prefix('[').unwrap())
                    .map_err(|e| e.to_string())?,
            )];

            // Process following args in array
            let mut array_end = false;
            while !array_end {
                if let Some(value) = input.next() {
                    // Last arg in array
                    if value.ends_with(']') {
                        array_arg.push(Arg::from(
                            Felt252::from_dec_str(value.strip_suffix(']').unwrap())
                                .map_err(|e| e.to_string())?,
                        ));
                        array_end = true;
                    } else {
                        array_arg.push(Arg::from(
                            Felt252::from_dec_str(value).map_err(|e| e.to_string())?,
                        ));
                    }
                }
            }
            // Finalize array
            args.push(Arg::Array(array_arg));
        } else {
            // Single argument
            args.push(Arg::from(
                Felt252::from_dec_str(value).map_err(|e| e.to_string())?,
            ));
        }
    }

    Ok(args)
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to extract arguments from the provided string")]
    BadArgumentStringFormat,
    #[error("Failed to interact with the file system")]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    EncodeTrace(#[from] EncodeTraceError),
    #[error(transparent)]
    VirtualMachine(#[from] VirtualMachineError),
    #[error(transparent)]
    Trace(#[from] TraceError),
    #[error(transparent)]
    PublicInput(#[from] PublicInputError),
    #[error(transparent)]
    Runner(#[from] RunnerError),
    #[error(transparent)]
    ProgramRegistry(#[from] Box<ProgramRegistryError>),
    #[error(transparent)]
    Compilation(#[from] Box<CompilationError>),
    #[error("Failed to compile to sierra:\n {0}")]
    SierraCompilation(String),
    #[error(transparent)]
    Metadata(#[from] MetadataError),
    #[error(transparent)]
    Program(#[from] ProgramError),
    #[error(transparent)]
    Memory(#[from] MemoryError),
    #[error("Program panicked with {0:?}")]
    RunPanic(Vec<Felt252>),
    #[error("Function signature has no return types")]
    NoRetTypesInSignature,
    #[error("No size for concrete type id: {0}")]
    NoTypeSizeForId(ConcreteTypeId),
    #[error("Concrete type id has no debug name: {0}")]
    TypeIdNoDebugName(ConcreteTypeId),
    #[error("No info in sierra program registry for concrete type id: {0}")]
    NoInfoForType(ConcreteTypeId),
    #[error("Failed to extract return values from VM")]
    FailedToExtractReturnValues,
    #[error("Function expects arguments of size {expected} and received {actual} instead.")]
    ArgumentsSizeMismatch { expected: i16, actual: i16 },
    #[error("Function param {param_index} only partially contains argument {arg_index}.")]
    ArgumentUnaligned {
        param_index: usize,
        arg_index: usize,
    },
    // TODO: find a better error form
    #[error("Compilation error")]
    DiagnosticsError(Vec<String>),
    #[error("Error occurred: {0}")]
    Anyhow(#[from] anyhow::Error),
}

pub struct FileWriter {
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
    fn new(buf_writer: io::BufWriter<std::fs::File>) -> Self {
        Self {
            buf_writer,
            bytes_written: 0,
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.buf_writer.flush()
    }
}

#[derive(Debug)]
pub enum RunOutput {
    Success(Option<String>),
    Panic(String),
}

#[derive(Default, Debug, Serialize, Clone)]
pub struct Costs {
    /// A compile time known cost unit. This is a linear combination of the runtime tokens
    /// (`step`, `hole`, `range_check`).
    pub const_cost: i64,
    // Runtime post-cost token types:
    /// The number of steps.
    pub step: i64,
    /// The number of memory holes (untouched memory addresses).
    pub hole: i64,
    /// The number of range check builtins.
    pub range_checks: i64,
    /// The number of range check builtins.
    pub range_checks96: i64,
    /// One invocation of the pedersen hash function.
    pub pedersen: i64,
    /// One invocation of the Poseidon hades permutation.
    pub poseidon: i64,
    /// One invocation of the bitwise builtin.
    pub bitwise: i64,
    /// One invocation of the EC op builtin.
    pub ec_op: i64,
    /// The AddMod op builtin.
    pub add_mod: i64,
    /// The MulMod op builtin.
    pub mul_mod: i64,
}

#[derive(Debug, Serialize)]
pub struct ProgramCosts {
    function_costs: HashMap<String, Costs>,
    variable_costs: HashMap<i64, Costs>,
    statements_costs: HashMap<i32, Vec<StatementCosts>>,
}

pub struct RunResult {
    pub output: RunOutput,
    pub trace: Vec<RelocatedTraceEntry>,
    pub memory: Vec<Option<Felt252>>,
    pub sierra_program_with_debug: SierraProgramWithDebug,
    pub casm_program: CairoProgram,
    pub instructions: Vec<Instruction>,
    pub headers_len: usize,
    pub diagnostics: Vec<String>,
    pub compiler_db: RootDatabase,
    pub costs: ProgramCosts,
}

pub fn run_program_at_path(filename: &Path, arguments_as_str: &str) -> Result<RunResult, Error> {
    let proof_mode = false;
    let trace_file: Option<PathBuf> = None;
    let air_public_input: Option<PathBuf> = None;
    let cairo_pie_output: Option<PathBuf> = None;
    let air_private_input: Option<PathBuf> = None;
    let memory_file: Option<PathBuf> = None;

    // configure diagnostics
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

    // extract program arguments
    let program_args =
        process_args(arguments_as_str).map_err(|_| Error::BadArgumentStringFormat)?;

    let compiler_config = CompilerConfig {
        replace_ids: true,
        diagnostics_reporter,
        ..CompilerConfig::default()
    };

    let mut db_builder = RootDatabase::builder();
    db_builder.detect_corelib();
    db_builder.with_cfg(CfgSet::from_iter([Cfg::kv("target", "test")]));
    db_builder.with_plugin_suite(test_plugin_suite());
    db_builder.with_plugin_suite(starknet_plugin_suite());
    let mut compiler_db = db_builder.build().unwrap();

    let main_crate_ids = setup_project(&mut compiler_db, filename).unwrap();

    let sierra_program_with_debug =
        compile_prepared_db(&compiler_db, main_crate_ids.clone(), compiler_config)
            .map_err(|_| Error::DiagnosticsError(program_diagnostics.clone()))?;

    let sierra_program = &sierra_program_with_debug.program;

    let metadata_config = Some(Default::default());
    let metadata: Metadata = create_metadata(sierra_program, metadata_config.clone())?;
    let sierra_program_registry: ProgramRegistry<CoreType, CoreLibfunc> =
        ProgramRegistry::<CoreType, CoreLibfunc>::new(sierra_program)?;
    let type_sizes =
        get_type_size_map(sierra_program, &sierra_program_registry).unwrap_or_default();

    let libfuncs_costs = get_libfuncs_costs(sierra_program, &sierra_program_registry, &metadata);

    let replacer = DebugReplacer { db: &compiler_db };
    let contracts_info = get_contracts_info(&compiler_db, main_crate_ids.clone(), &replacer)?;

    let sierra_casm_runner = SierraCasmRunner::new(
        sierra_program.clone(),
        metadata_config,
        contracts_info,
        None,
    )
    .map_err(|_| {
        Error::Anyhow(anyhow::anyhow!(
            "Error enabling running a Sierra program on the vm.".to_string()
        ))
    })?;

    let casm_program = sierra_casm_runner.get_casm_program();

    let main_func = find_function(sierra_program, "::main")?;
    let initial_gas = 9999999999999_usize;

    // Modified entry code to be compatible with custom cairo1 Proof Mode.
    // This adds code that's needed for dictionaries, adjusts ap for builtin pointers, adds initial gas for the gas builtin if needed, and sets up other necessary code for cairo1
    let (entry_code, builtins) = sierra_casm_runner
        .create_entry_code(main_func, &program_args, initial_gas)
        .map_err(|_| {
            Error::Anyhow(anyhow::anyhow!(
                "Error while creating entry code".to_string()
            ))
        })?;

    // Get the user program instructions
    let program_instructions = casm_program.instructions.iter();

    // This footer is used by lib funcs
    let libfunc_footer = create_code_footer();

    let proof_mode_header = if proof_mode {
        // Prepare "canonical" proof mode instructions. These are usually added by the compiler in cairo 0
        let mut ctx = casm! {};
        casm_extend! {ctx,
            call rel 4;
            jmp rel 0;
        };
        ctx.instructions
    } else {
        casm! {}.instructions
    };

    // This is the program we are actually running/proving
    // With (embedded proof mode), cairo1 header and the libfunc footer
    let instructions = chain!(
        proof_mode_header.iter(),
        entry_code.iter(),
        program_instructions,
        libfunc_footer.iter()
    );

    let instructions_vec: Vec<Instruction> = proof_mode_header
        .iter()
        .chain(entry_code.iter())
        .chain(casm_program.instructions.iter())
        .chain(libfunc_footer.iter())
        .cloned()
        .collect();
    let headers_len = proof_mode_header.len() + entry_code.len();

    let (hints_dict, string_to_hint) = build_hints_dict(instructions.clone());

    let mut hint_processor = CairoHintProcessor {
        runner: Some(&sierra_casm_runner),
        starknet_state: StarknetState::default(),
        string_to_hint,
        run_resources: RunResources::default(),
        syscalls_used_resources: Default::default(),
    };
    let data: Vec<MaybeRelocatable> = instructions
        .flat_map(|inst| inst.assemble().encode())
        .map(|x| MaybeRelocatable::from(Felt252::from(&x)))
        .collect();

    let data_len = data.len();

    let program = if proof_mode {
        Program::new_for_proof(
            builtins,
            data,
            0,
            // Proof mode is on top
            // jmp rel 0 is on PC == 2
            2,
            hints_dict,
            ReferenceManager {
                references: Vec::new(),
            },
            HashMap::new(),
            vec![],
            None,
        )?
    } else {
        Program::new(
            builtins,
            data,
            Some(0),
            hints_dict,
            ReferenceManager {
                references: Vec::new(),
            },
            HashMap::new(),
            vec![],
            None,
        )?
    };

    let runner_mode = if proof_mode {
        RunnerMode::ProofModeCanonical
    } else {
        RunnerMode::ExecutionMode
    };

    let mut runner = CairoRunner::new_v2(&program, LayoutName::all_cairo, runner_mode, true)?;
    let end = runner.initialize(true)?;

    additional_initialization(&mut runner.vm, data_len)?;

    // Run it until the end/ infinite loop in proof_mode
    runner.run_until_pc(end, &mut hint_processor)?;
    runner.end_run(false, false, &mut hint_processor)?;

    let mut panic_output: Option<String> = None;
    let mut output_string: Option<String> = None;

    // Fetch return type data
    if let Some(return_type_id) = main_func.signature.ret_types.last() {
        let return_type_size = type_sizes
            .get(return_type_id)
            .cloned()
            .ok_or_else(|| Error::NoTypeSizeForId(return_type_id.clone()))?;

        let mut return_values = runner.vm.get_return_values(return_type_size as usize)?;

        // Check if this result is a Panic result
        if return_type_id
            .debug_name
            .as_ref()
            .ok_or_else(|| Error::TypeIdNoDebugName(return_type_id.clone()))?
            .starts_with("core::panics::PanicResult::")
        {
            // Check the failure flag (aka first return value)
            if return_values.first() != Some(&MaybeRelocatable::from(0)) {
                // In case of failure, extract the error from the return values (aka last two values)
                let panic_data_end = return_values
                    .last()
                    .ok_or(Error::FailedToExtractReturnValues)?
                    .get_relocatable()
                    .ok_or(Error::FailedToExtractReturnValues)?;
                let panic_data_start = return_values
                    .get(return_values.len() - 2)
                    .ok_or(Error::FailedToExtractReturnValues)?
                    .get_relocatable()
                    .ok_or(Error::FailedToExtractReturnValues)?;
                let panic_data = runner.vm.get_integer_range(
                    panic_data_start,
                    (panic_data_end - panic_data_start).map_err(VirtualMachineError::Math)?,
                )?;

                let panic_data_felts: Vec<Felt252> =
                    panic_data.iter().map(|c| *c.as_ref()).collect();
                panic_output = Some(
                    panic_data_felts
                        .iter()
                        .map(|r| bytes_to_text(r.to_bytes_be()))
                        .collect::<Result<Vec<String>, _>>()
                        .map(|v| v.join(""))
                        .ok()
                        .unwrap_or("Failed to extract panic error".to_string()),
                );
                // return Err(Error::RunPanic(
                //     panic_data.iter().map(|c| *c.as_ref()).collect(),
                // ));
            } else {
                if return_values.len() < 3 {
                    return Err(Error::FailedToExtractReturnValues);
                }
                return_values = return_values[2..].to_vec()
            }
        }
        output_string = Some(serialize_output(&runner.vm, &return_values));
    }

    // Set stop pointers for builtins so we can obtain the air public input
    if air_public_input.is_some() || cairo_pie_output.is_some() {
        // Cairo 1 programs have other return values aside from the used builtin's final pointers, so we need to hand-pick them
        let ret_types_sizes = main_func
            .signature
            .ret_types
            .iter()
            .map(|id| type_sizes.get(id).cloned().unwrap_or_default());
        let ret_types_and_sizes = main_func
            .signature
            .ret_types
            .iter()
            .zip(ret_types_sizes.clone());

        let full_ret_types_size: i16 = ret_types_sizes.sum();
        let mut stack_pointer = (runner.vm.get_ap()
            - (full_ret_types_size as usize).saturating_sub(1))
        .map_err(VirtualMachineError::Math)?;

        // Calculate the stack_ptr for each return builtin in the return values
        let mut builtin_name_to_stack_pointer = HashMap::new();
        for (id, size) in ret_types_and_sizes {
            if let Some(ref name) = id.debug_name {
                let builtin_name = match &*name.to_string() {
                    "RangeCheck" => BuiltinName::range_check,
                    "Poseidon" => BuiltinName::poseidon,
                    "EcOp" => BuiltinName::ec_op,
                    "Bitwise" => BuiltinName::bitwise,
                    "Pedersen" => BuiltinName::pedersen,
                    "Output" => BuiltinName::output,
                    "Ecdsa" => BuiltinName::ecdsa,
                    "Keccak" => BuiltinName::keccak,
                    "SegmentArena" => BuiltinName::segment_arena,
                    "RangeCheck96" => BuiltinName::range_check96,
                    "AddMod" => BuiltinName::add_mod,
                    "MulMod" => BuiltinName::mul_mod,
                    _ => {
                        stack_pointer.offset += size as usize;
                        continue;
                    }
                };
                builtin_name_to_stack_pointer.insert(builtin_name, stack_pointer);
            }
            stack_pointer.offset += size as usize;
        }
        // Set stop pointer for each builtin
        runner
            .vm
            .builtins_final_stack_from_stack_pointer_dict(&builtin_name_to_stack_pointer, false)?;

        // Build execution public memory
        if proof_mode {
            runner.finalize_segments()?;
        }
    }

    runner.relocate(true)?;

    if let Some(file_path) = air_public_input {
        let json = runner.get_air_public_input()?.serialize_json()?;
        std::fs::write(file_path, json)?;
    }

    if let (Some(file_path), Some(trace_file), Some(memory_file)) =
        (air_private_input, trace_file.clone(), memory_file.clone())
    {
        // Get absolute paths of trace_file & memory_file
        let trace_path = trace_file
            .as_path()
            .canonicalize()
            .unwrap_or(trace_file.clone())
            .to_string_lossy()
            .to_string();
        let memory_path = memory_file
            .as_path()
            .canonicalize()
            .unwrap_or(memory_file.clone())
            .to_string_lossy()
            .to_string();

        let json = runner
            .get_air_private_input()
            .to_serializable(trace_path, memory_path)
            .serialize_json()
            .map_err(PublicInputError::Serde)?;
        std::fs::write(file_path, json)?;
    }

    if let Some(ref file_path) = cairo_pie_output {
        runner.get_cairo_pie()?.write_zip_file(file_path)?
    }

    let relocated_trace = runner
        .relocated_trace
        .ok_or(Error::Trace(TraceError::TraceNotRelocated))?;

    if let Some(trace_path) = trace_file {
        // let relocated_trace = runner
        //     .relocated_trace
        //     .ok_or(Error::Trace(TraceError::TraceNotRelocated))?;
        let trace_file = std::fs::File::create(trace_path)?;
        let mut trace_writer =
            FileWriter::new(io::BufWriter::with_capacity(3 * 1024 * 1024, trace_file));

        cairo_run::write_encoded_trace(&relocated_trace, &mut trace_writer)?;
        trace_writer.flush()?;
    }

    // let relocated_memory = runner.relocated_memory;

    if let Some(memory_path) = memory_file {
        let memory_file = std::fs::File::create(memory_path)?;
        let mut memory_writer =
            FileWriter::new(io::BufWriter::with_capacity(5 * 1024 * 1024, memory_file));

        cairo_run::write_encoded_memory(&runner.relocated_memory, &mut memory_writer)?;
        memory_writer.flush()?;
    }
    let mut function_costs: HashMap<String, Costs> = HashMap::new();
    let mut variable_costs: HashMap<i64, Costs> = HashMap::new();
    for ((stm_idx, token_type), v) in metadata.gas_info.variable_values.iter() {
        let index: i64 = stm_idx.0.try_into().expect("This shouldn't fail.");
        variable_costs.entry(index).or_insert_with(|| {
            let costs: Costs = Costs::default();
            costs
        });
        let costs = variable_costs.get_mut(&index).unwrap();
        match token_type {
            CostTokenType::Const => {
                costs.const_cost = v.into_or_panic();
            }
            CostTokenType::Step => {
                costs.step = v.into_or_panic();
            }
            CostTokenType::Hole => {
                costs.hole = v.into_or_panic();
            }
            CostTokenType::RangeCheck => {
                costs.range_checks = v.into_or_panic();
            }
            CostTokenType::RangeCheck96 => {
                costs.range_checks96 = v.into_or_panic();
            }
            CostTokenType::Pedersen => {
                costs.pedersen = v.into_or_panic();
            }
            CostTokenType::Poseidon => {
                costs.poseidon = v.into_or_panic();
            }
            CostTokenType::Bitwise => {
                costs.bitwise = v.into_or_panic();
            }
            CostTokenType::EcOp => {
                costs.ec_op = v.into_or_panic();
            }
            CostTokenType::AddMod => {
                costs.add_mod = v.into_or_panic();
            }
            CostTokenType::MulMod => {
                costs.mul_mod = v.into_or_panic();
            }
        }
    }
    for (function_id, fc) in metadata.gas_info.function_costs.iter() {
        let mut costs: Costs = Costs::default();
        for (token_type, v) in fc.iter() {
            match token_type {
                CostTokenType::Const => {
                    costs.const_cost = v.into_or_panic();
                }
                CostTokenType::Step => {
                    costs.step = v.into_or_panic();
                }
                CostTokenType::Hole => {
                    costs.hole = v.into_or_panic();
                }
                CostTokenType::RangeCheck => {
                    costs.range_checks = v.into_or_panic();
                }
                CostTokenType::RangeCheck96 => {
                    costs.range_checks96 = v.into_or_panic();
                }
                CostTokenType::Pedersen => {
                    costs.pedersen = v.into_or_panic();
                }
                CostTokenType::Poseidon => {
                    costs.poseidon = v.into_or_panic();
                }
                CostTokenType::Bitwise => {
                    costs.bitwise = v.into_or_panic();
                }
                CostTokenType::EcOp => {
                    costs.ec_op = v.into_or_panic();
                }
                CostTokenType::AddMod => {
                    costs.add_mod = v.into_or_panic();
                }
                CostTokenType::MulMod => {
                    costs.mul_mod = v.into_or_panic();
                }
            }
        }

        function_costs.insert(function_id.to_string(), costs);
    }

    Ok(RunResult {
        output: match panic_output {
            Some(panic_output) => RunOutput::Panic(panic_output),
            None => RunOutput::Success(output_string),
        },
        trace: relocated_trace,
        memory: runner.relocated_memory,
        sierra_program_with_debug,
        casm_program: casm_program.clone(),
        instructions: instructions_vec,
        headers_len,
        diagnostics: program_diagnostics,
        compiler_db,
        costs: ProgramCosts {
            function_costs,
            variable_costs,
            statements_costs: libfuncs_costs.unwrap(),
        },
    })
}

// copied and modified
// cairo/crates/cairo-lang-runner/src/lib.rs:134
// cairo/crates/cairo-lang-sierra-gas/src/objects.rs:15
pub fn token_gas_cost(token_type: CostTokenType) -> usize {
    match token_type {
        CostTokenType::Const => 1,
        CostTokenType::Step
        | CostTokenType::Hole
        | CostTokenType::RangeCheck
        | CostTokenType::RangeCheck96 => {
            panic!("Token type {:?} has no gas cost.", token_type)
        }
        CostTokenType::Pedersen => 4050,
        CostTokenType::Poseidon => 491,
        CostTokenType::Bitwise => 583,
        CostTokenType::EcOp => 4085,
        CostTokenType::AddMod => 230,
        CostTokenType::MulMod => 604,
    }
}

fn additional_initialization(vm: &mut VirtualMachine, data_len: usize) -> Result<(), Error> {
    // Create the builtin cost segment
    let builtin_cost_segment = vm.add_memory_segment();
    for token_type in CostTokenType::iter_precost() {
        vm.insert_value(
            (builtin_cost_segment + (token_type.offset_in_builtin_costs() as usize))
                .map_err(VirtualMachineError::Math)?,
            Felt252::default(),
        )?
    }
    // Put a pointer to the builtin cost segment at the end of the program (after the
    // additional `ret` statement).
    vm.insert_value(
        (vm.get_pc() + data_len).map_err(VirtualMachineError::Math)?,
        builtin_cost_segment,
    )?;

    Ok(())
}

// fn main() -> Result<(), Error> {
//     match run(std::env::args()) {
//         Err(Error::Cli(err)) => err.exit(),
//         Ok(output) => {
//             if let Some(output_string) = output {
//                 println!("Program Output : {}", output_string);
//             }
//             Ok(())
//         }
//         Err(Error::RunPanic(panic_data)) => {
//             if !panic_data.is_empty() {
//                 let panic_data_string_list = panic_data
//                     .iter()
//                     .map(|m| {
//                         // Try to parse to utf8 string
//                         let msg = String::from_utf8(m.to_bytes_be().to_vec());
//                         if let Ok(msg) = msg {
//                             format!("{} ('{}')", m, msg)
//                         } else {
//                             m.to_string()
//                         }
//                     })
//                     .join(", ");
//                 println!("Run panicked with: [{}]", panic_data_string_list);
//             }
//             Ok(())
//         }
//         Err(err) => Err(err),
//     }
// }

/// Finds first function ending with `name_suffix`.
fn find_function<'a>(
    sierra_program: &'a SierraProgram,
    name_suffix: &'a str,
) -> Result<&'a Function, RunnerError> {
    sierra_program
        .funcs
        .iter()
        .find(|f| {
            if let Some(name) = &f.id.debug_name {
                name.ends_with(name_suffix)
            } else {
                false
            }
        })
        .ok_or_else(|| RunnerError::MissingMain)
}

/// Creates a list of instructions that will be appended to the program's bytecode.
fn create_code_footer() -> Vec<Instruction> {
    casm! {
        // Add a `ret` instruction used in libfuncs that retrieve the current value of the `fp`
        // and `pc` registers.
        ret;
    }
    .instructions
}

/// Creates the metadata required for a Sierra program lowering to casm.
fn create_metadata(
    sierra_program: &cairo_lang_sierra::program::Program,
    metadata_config: Option<MetadataComputationConfig>,
) -> Result<Metadata, VirtualMachineError> {
    if let Some(metadata_config) = metadata_config {
        calc_metadata(sierra_program, metadata_config).map_err(|err| match err {
            MetadataError::ApChangeError(_) => VirtualMachineError::Unexpected,
            MetadataError::CostError(_) => VirtualMachineError::Unexpected,
        })
    } else {
        Ok(Metadata {
            ap_change_info: calc_ap_changes(sierra_program, |_, _| 0)
                .map_err(|_| VirtualMachineError::Unexpected)?,
            gas_info: GasInfo {
                variable_values: Default::default(),
                function_costs: Default::default(),
            },
        })
    }
}

#[derive(Debug, Serialize)]
enum StatementType {
    Return,
    Invocation,
    Unknown,
}

#[derive(Debug, Serialize)]
pub struct StatementCosts {
    statement_type: StatementType,
    statement_index: i32,
    costs: Costs,
}

struct InvocationCostInfoProviderForCosts<
    'a,
    TokenUsages: Fn(CostTokenType) -> usize,
    ApChangeVarValue: Fn() -> usize,
> {
    /// Registry for providing the sizes of the types.
    type_sizes: &'a TypeSizeMap,
    /// Closure providing the token usages for the invocation.
    token_usages: TokenUsages,
    /// Closure providing the ap changes for the invocation.
    ap_change_var_value: ApChangeVarValue,
}

impl<'a, TokenUsages: Fn(CostTokenType) -> usize, ApChangeVarValue: Fn() -> usize>
    InvocationCostInfoProvider
    for InvocationCostInfoProviderForCosts<'a, TokenUsages, ApChangeVarValue>
{
    fn type_size(&self, ty: &ConcreteTypeId) -> usize {
        self.type_sizes[ty].into_or_panic()
    }

    fn token_usages(&self, token_type: CostTokenType) -> usize {
        (self.token_usages)(token_type)
    }

    fn ap_change_var_value(&self) -> usize {
        (self.ap_change_var_value)()
    }
    fn circuit_info(
        &self,
        _ty: &ConcreteTypeId,
    ) -> &cairo_lang_sierra::extensions::circuit::CircuitInfo {
        todo!()
    }
}
pub fn get_libfuncs_costs(
    sierra_program: &SierraProgram,
    sierra_program_registry: &ProgramRegistry<CoreType, CoreLibfunc>,
    metadata: &Metadata,
) -> Result<HashMap<i32, Vec<StatementCosts>>, Error> {
    let type_size_map = get_type_size_map(sierra_program, sierra_program_registry).unwrap();
    let mut statements_costs: HashMap<i32, Vec<StatementCosts>> = HashMap::new();
    for i in 0..sierra_program.clone().statements.len() {
        match sierra_program.get_statement(&StatementIdx(i)).unwrap() {
            program::GenStatement::Invocation(invocation) => {
                let core_libfunc = sierra_program_registry
                    .get_libfunc(&invocation.libfunc_id)
                    .expect("Program registry creation would have already failed.");
                let libfunc_costs = libfunc_cost(
                    &metadata.gas_info,
                    &StatementIdx(i),
                    core_libfunc,
                    &InvocationCostInfoProviderForCosts {
                        type_sizes: &type_size_map,
                        token_usages: |token_type| {
                            metadata
                                .gas_info
                                .variable_values
                                .get(&(StatementIdx(i), token_type))
                                .copied()
                                .unwrap_or(0) as usize
                        },
                        ap_change_var_value: || {
                            metadata
                                .ap_change_info
                                .variable_values
                                .get(&StatementIdx(i))
                                .copied()
                                .unwrap_or_default()
                        },
                    },
                );
                let costs = Costs::default();
                if libfunc_costs.is_empty() {
                    statements_costs.insert(
                        i.into_or_panic(),
                        vec![StatementCosts {
                            statement_type: StatementType::Unknown,
                            statement_index: i.into_or_panic(),
                            costs: costs.clone(),
                        }],
                    );
                    continue;
                }
                let mut costs_vec: Vec<StatementCosts> = vec![];
                for cost in libfunc_costs {
                    match cost {
                        Some(c) => {
                            let mut branch_costs = Costs::default();
                            for (cost_type, value) in c.clone().iter() {
                                update_costs(&mut branch_costs, cost_type, *value);
                            }
                            costs_vec.push(StatementCosts {
                                statement_type: StatementType::Invocation,
                                statement_index: i.into_or_panic(),
                                costs: branch_costs.clone(),
                            })
                        }
                        None => {
                            costs_vec.push(StatementCosts {
                                statement_type: StatementType::Unknown,
                                statement_index: i.into_or_panic(),
                                costs: Costs::default(),
                            });
                            continue;
                        }
                    }
                }

                statements_costs.insert(i.into_or_panic(), costs_vec);
            }
            program::GenStatement::Return(_) => {
                statements_costs.insert(
                    i.into_or_panic(),
                    vec![StatementCosts {
                        statement_type: StatementType::Return,
                        statement_index: i.into_or_panic(),
                        costs: Costs::default(),
                    }],
                );
            }
        }
    }
    Ok(statements_costs)
}

fn update_costs(costs: &mut Costs, c: &CostTokenType, v: i64) {
    match c {
        CostTokenType::Pedersen => costs.pedersen = v,
        CostTokenType::Poseidon => {
            costs.poseidon = v;
        }
        CostTokenType::Bitwise => {
            costs.bitwise = v;
        }
        CostTokenType::EcOp => costs.ec_op = v,
        CostTokenType::Const => costs.const_cost = v,
        CostTokenType::Step => costs.step = v,
        CostTokenType::Hole => costs.hole = v,
        CostTokenType::RangeCheck => costs.range_checks = v,
        CostTokenType::RangeCheck96 => costs.range_checks96 = v,
        CostTokenType::AddMod => costs.add_mod = v,
        CostTokenType::MulMod => costs.mul_mod = v,
    }
}

fn serialize_output(vm: &VirtualMachine, return_values: &[MaybeRelocatable]) -> String {
    let mut output_string = String::new();
    let mut return_values_iter: Peekable<Iter<MaybeRelocatable>> = return_values.iter().peekable();
    serialize_output_inner(&mut return_values_iter, &mut output_string, vm);
    fn serialize_output_inner(
        iter: &mut Peekable<Iter<MaybeRelocatable>>,
        output_string: &mut String,
        vm: &VirtualMachine,
    ) {
        while let Some(val) = iter.next() {
            if let MaybeRelocatable::RelocatableValue(x) = val {
                // Check if the next value is a relocatable of the same index
                if let Some(MaybeRelocatable::RelocatableValue(y)) = iter.peek() {
                    // Check if the two relocatable values represent a valid array in memory
                    if x.segment_index == y.segment_index && x.offset <= y.offset {
                        // Fetch the y value from the iterator so we don't serialize it twice
                        iter.next();
                        // Fetch array
                        maybe_add_whitespace(output_string);
                        output_string.push('[');
                        let array = vm.get_continuous_range(*x, y.offset - x.offset).unwrap();
                        let mut array_iter: Peekable<Iter<MaybeRelocatable>> =
                            array.iter().peekable();
                        serialize_output_inner(&mut array_iter, output_string, vm);
                        output_string.push(']');
                        continue;
                    }
                }
            }
            maybe_add_whitespace(output_string);
            output_string.push_str(&val.to_string());
        }
    }

    fn maybe_add_whitespace(string: &mut String) {
        if !string.is_empty() && !string.ends_with('[') {
            string.push(' ');
        }
    }
    output_string
}

fn bytes_to_text(bytes: [u8; 32]) -> Result<String, std::str::Utf8Error> {
    let mut text = std::str::from_utf8(&bytes)?.to_string();
    text.retain(|c| c != '\0');
    Ok(text)
}
