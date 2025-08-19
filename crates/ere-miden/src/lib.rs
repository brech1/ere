//! Ere Miden zkVM interface.

pub mod error;

use error::{CompileError, MidenError};
use miden_assembly::{Assembler, DefaultSourceManager};
use miden_core::{
    Program,
    utils::{Deserializable, Serializable},
};
use miden_processor::{
    AdviceInputs, DefaultHost, ExecutionOptions, ProgramInfo, StackInputs, StackOutputs,
};
use miden_prover::{ExecutionProof, ProvingOptions};
use miden_stdlib::StdLibrary;
use miden_verifier::verify as miden_verify;
use serde::{Deserialize, Serialize};
use std::{env, path::Path, sync::Arc, time::Instant};
use tracing::info;
use zkvm_interface::{
    Compiler, Input, InputItem, ProgramExecutionReport, ProgramProvingReport, ProverResourceType,
    zkVM, zkVMError,
};

include!(concat!(env!("OUT_DIR"), "/name_and_sdk_version.rs"));

/// Miden target for compiling Miden Assembly programs.
pub struct MidenTarget;

/// Miden program struct.
#[derive(Clone, Serialize, Deserialize)]
pub struct MidenProgram {
    pub program_bytes: Vec<u8>,
}

impl Compiler for MidenTarget {
    type Error = MidenError;
    type Program = MidenProgram;

    fn compile(&self, guest_directory: &Path) -> Result<Self::Program, Self::Error> {
        let main_path = guest_directory.join("main.masm");
        if !main_path.exists() {
            return Err(CompileError::InvalidSource(format!(
                "Expected main entrypoint: {}",
                main_path.display()
            ))
            .into());
        }

        let source_manager = Arc::new(DefaultSourceManager::default());
        let mut assembler = Assembler::new(source_manager).with_debug_mode(false);

        assembler
            .link_dynamic_library(StdLibrary::default())
            .map_err(|e| CompileError::InvalidSource(format!("Failed to load stdlib: {e}")))?;

        // Assemble the program directly from the file path
        let program = assembler
            .assemble_program(main_path)
            .map_err(|e| CompileError::InvalidSource(e.to_string()))?;

        Ok(MidenProgram {
            program_bytes: program.to_bytes(),
        })
    }
}

/// Ere Miden zkVM instance.
pub struct EreMiden {
    program: Program,
}

impl EreMiden {
    pub fn new(program: MidenProgram, resource: ProverResourceType) -> Self {
        assert!(
            matches!(resource, ProverResourceType::Cpu),
            "Miden backend only supports ProverResourceType::Cpu"
        );

        let program =
            Program::read_from_bytes(&program.program_bytes).expect("Valid Miden program bytes");

        Self { program }
    }
}

impl zkVM for EreMiden {
    fn execute(&self, inputs: &Input) -> Result<ProgramExecutionReport, zkVMError> {
        let (stack_inputs, advice_inputs, _) =
            prepare_inputs(inputs).map_err(|e| zkVMError::Other(e.to_string().into()))?;

        let mut host = DefaultHost::default();
        host.load_library(&StdLibrary::default())
            .map_err(|e| zkVMError::Other(Box::new(e)))?;

        let start = Instant::now();
        let trace = miden_processor::execute(
            &self.program,
            stack_inputs,
            advice_inputs,
            &mut host,
            ExecutionOptions::default(),
        )
        .map_err(|e| zkVMError::Other(Box::new(e)))?;

        let cycles = trace.trace_len_summary().main_trace_len() as u64;
        Ok(ProgramExecutionReport {
            total_num_cycles: cycles,
            execution_duration: start.elapsed(),
            ..Default::default()
        })
    }

    fn prove(&self, inputs: &Input) -> Result<(Vec<u8>, ProgramProvingReport), zkVMError> {
        info!("Generating Miden proof…");
        let (stack_inputs, advice_inputs, raw_stack_inputs) =
            prepare_inputs(inputs).map_err(|e| zkVMError::Other(e.to_string().into()))?;

        let mut host = DefaultHost::default();
        host.load_library(&StdLibrary::default())
            .map_err(|e| zkVMError::Other(Box::new(e)))?;

        let start = Instant::now();
        let (outputs, proof) = miden_prover::prove(
            &self.program,
            stack_inputs,
            advice_inputs,
            &mut host,
            ProvingOptions::with_96_bit_security(false),
        )
        .map_err(|e| zkVMError::Other(Box::new(e)))?;

        let bundle = MidenProofBundle {
            inputs: raw_stack_inputs,
            outputs: outputs.iter().map(|f| f.as_int()).collect(),
            proof: proof.to_bytes(),
        };
        let bytes = bincode::serialize(&bundle).map_err(|e| zkVMError::Other(Box::new(e)))?;

        Ok((bytes, ProgramProvingReport::new(start.elapsed())))
    }

    fn verify(&self, proof: &[u8]) -> Result<(), zkVMError> {
        info!("Verifying Miden proof…");
        let bundle: MidenProofBundle =
            bincode::deserialize(proof).map_err(|e| zkVMError::Other(Box::new(e)))?;

        let program_info: ProgramInfo = self.program.clone().into();
        let stack_inputs =
            StackInputs::try_from_ints(bundle.inputs).map_err(|e| zkVMError::Other(Box::new(e)))?;
        let stack_outputs = StackOutputs::try_from_ints(bundle.outputs)
            .map_err(|e| zkVMError::Other(Box::new(e)))?;
        let proof =
            ExecutionProof::from_bytes(&bundle.proof).map_err(|e| zkVMError::Other(Box::new(e)))?;

        miden_verify(program_info, stack_inputs, stack_outputs, proof)
            .map(|_security_level| ())
            .map_err(|e| zkVMError::Other(Box::new(e)))
    }

    fn name(&self) -> &'static str {
        NAME
    }
    fn sdk_version(&self) -> &'static str {
        SDK_VERSION
    }
}

#[derive(Serialize, Deserialize)]
pub struct MidenProofBundle {
    inputs: Vec<u64>,
    outputs: Vec<u64>,
    proof: Vec<u8>,
}

pub fn prepare_inputs(inputs: &Input) -> anyhow::Result<(StackInputs, AdviceInputs, Vec<u64>)> {
    let mut stack_vals = Vec::new();
    let mut advice_tape_vals = Vec::new();

    for item in inputs.iter() {
        let bytes = match item {
            InputItem::Bytes(bytes) => bytes.clone(),
            InputItem::SerializedObject(bytes) => bytes.clone(),
            InputItem::Object(obj) => bincode::serialize(obj)?,
        };

        // Since ere `Input` is just a Vec<u8>, I just push the length of the object to the stack.
        // The rest of the object will be pushed to the advice.
        // Stack can only hold 16 elements.

        stack_vals.push(bytes.len() as u64);

        let mut padded_bytes = bytes;
        let remainder = padded_bytes.len() % 4;
        if remainder != 0 {
            let padding_len = 4 - remainder;
            padded_bytes.extend(vec![0; padding_len]);
        }

        let words: Vec<u64> = padded_bytes
            .chunks_exact(4)
            .map(|chunk| u32::from_be_bytes(chunk.try_into().unwrap()) as u64)
            .collect();
        advice_tape_vals.extend(words);
    }

    let stack_inputs = StackInputs::try_from_ints(stack_vals.clone())?;
    let advice_inputs = AdviceInputs::default().with_stack_values(advice_tape_vals)?;

    Ok((stack_inputs, advice_inputs, stack_vals))
}
