use std::{path::Path, sync::Arc, time::Instant};

use miden_assembly::{
    Assembler, DefaultSourceManager, LibraryNamespace, utils::Deserializable, utils::Serializable,
};
use miden_core::{Program, StackInputs};
use miden_processor::{DefaultHost, ExecutionOptions};
use miden_prover::{AdviceInputs, ExecutionProof, ProvingOptions, StackOutputs};
use miden_stdlib::StdLibrary;
use miden_verifier::{ProgramInfo, verify as miden_verify};
use serde::{Deserialize, Serialize};
use tracing as _;
use zkvm_interface::{
    Compiler, Input, InputItem, ProgramExecutionReport, ProgramProvingReport, ProverResourceType,
    zkVM, zkVMError,
};

include!(concat!(env!("OUT_DIR"), "/name_and_sdk_version.rs"));

mod error;
use error::{CompileError, ExecuteError, MidenError, ProveError, VerifyError};

#[allow(non_camel_case_types)]
pub struct MIDEN_TARGET;

/// Binary-serializable program artifact for Miden.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct MidenProgram {
    pub program_bytes: Vec<u8>,
}

impl Compiler for MIDEN_TARGET {
    type Error = MidenError;
    type Program = MidenProgram;

    fn compile(&self, guest_directory: &Path) -> Result<Self::Program, Self::Error> {
        // Expect a single MASM source file named main.masm under the guest directory for now.
        let main_path = guest_directory.join("main.masm");
        if !main_path.exists() {
            return Err(
                CompileError::InvalidSource(format!("Expected {}", main_path.display())).into(),
            );
        }

        let source_manager = Arc::new(DefaultSourceManager::default());
        let mut assembler = Assembler::new(source_manager.clone()).with_debug_mode(false);
        // Link stdlib dynamically by default
        assembler
            .link_dynamic_library(StdLibrary::default())
            .map_err(|e| CompileError::InvalidSource(format!("load stdlib: {e}")))?;

        // Use exec namespace for an executable
        let program: Program =
            miden_assembly::ast::Module::parser(miden_assembly::ast::ModuleKind::Executable)
                .parse_file(LibraryNamespace::Exec.into(), &main_path, &source_manager)
                .and_then(|ast| assembler.assemble_program(ast))
                .map_err(|e| CompileError::InvalidSource(format!("assemble: {e}")))?;

        let program_bytes = program.to_bytes();
        Ok(MidenProgram { program_bytes })
    }
}

pub struct EreMiden {
    program: Program,
    #[allow(dead_code)]
    resource: ProverResourceType,
}

impl EreMiden {
    pub fn new(program: <MIDEN_TARGET as Compiler>::Program, resource: ProverResourceType) -> Self {
        match &resource {
            ProverResourceType::Cpu => {}
            ProverResourceType::Gpu => panic!(
                "Miden backend does not support GPU proving yet; use ProverResourceType::Cpu"
            ),
            ProverResourceType::Network(_) => panic!(
                "Miden backend does not support network proving yet; use ProverResourceType::Cpu"
            ),
        }
        let program = <Program as Deserializable>::read_from_bytes(&program.program_bytes)
            .expect("valid Miden program bytes");
        Self { program, resource }
    }
}

impl zkVM for EreMiden {
    fn execute(&self, inputs: &Input) -> Result<ProgramExecutionReport, zkVMError> {
        let (stack_inputs, advice_inputs, _raw_inputs) = input_to_miden(inputs).map_err(|e| {
            zkVMError::Other(Box::new(ExecuteError::Client(format!("inputs: {e}"))))
        })?;

        let mut host = DefaultHost::default();
        let start = Instant::now();

        let trace = miden_processor::execute(
            &self.program,
            stack_inputs.clone(),
            advice_inputs,
            &mut host,
            ExecutionOptions::default(),
        )
        .map_err(|e| zkVMError::Other(Box::new(ExecuteError::Client(e.to_string()))))?;

        let len_summary = trace.trace_len_summary().clone();
        Ok(ProgramExecutionReport {
            total_num_cycles: len_summary.main_trace_len() as u64,
            region_cycles: Default::default(),
            execution_duration: start.elapsed(),
        })
    }

    fn prove(&self, inputs: &Input) -> Result<(Vec<u8>, ProgramProvingReport), zkVMError> {
        let (stack_inputs, advice_inputs, raw_inputs) = input_to_miden(inputs)
            .map_err(|e| zkVMError::Other(Box::new(ProveError::Client(format!("inputs: {e}")))))?;

        let mut host = DefaultHost::default();
        let now = Instant::now();
        let (outputs, proof): (StackOutputs, ExecutionProof) = miden_prover::prove(
            &self.program,
            stack_inputs,
            advice_inputs,
            &mut host,
            ProvingOptions::default(),
        )
        .map_err(|e| zkVMError::Other(Box::new(ProveError::Client(e.to_string()))))?;

        let elapsed = now.elapsed();
        let bundle = MidenProofBundle {
            inputs: raw_inputs,
            outputs: outputs.iter().map(|f| f.as_int()).collect(),
            proof: proof.to_bytes(),
        };
        let bytes = bincode::serialize(&bundle)
            .map_err(|e| zkVMError::Other(Box::new(ProveError::Client(e.to_string()))))?;
        Ok((bytes, ProgramProvingReport::new(elapsed)))
    }

    fn verify(&self, proof: &[u8]) -> Result<(), zkVMError> {
        let bundle: MidenProofBundle = bincode::deserialize(proof)
            .map_err(|e| zkVMError::Other(Box::new(VerifyError::Client(e.to_string()))))?;

        let program_info = ProgramInfo::from(self.program.clone());
        let stack_inputs = StackInputs::try_from_ints(bundle.inputs)
            .map_err(|e| zkVMError::Other(Box::new(VerifyError::Client(e.to_string()))))?;
        let stack_outputs = StackOutputs::try_from_ints(bundle.outputs)
            .map_err(|e| zkVMError::Other(Box::new(VerifyError::Client(e.to_string()))))?;
        let proof = ExecutionProof::from_bytes(&bundle.proof)
            .map_err(|e| zkVMError::Other(Box::new(VerifyError::Client(e.to_string()))))?;
        miden_verify(program_info, stack_inputs, stack_outputs, proof)
            .map(|_| ())
            .map_err(|e| zkVMError::Other(Box::new(VerifyError::Client(e.to_string()))))
    }

    fn name(&self) -> &'static str {
        NAME
    }

    fn sdk_version(&self) -> &'static str {
        SDK_VERSION
    }
}

#[derive(Serialize, Deserialize)]
struct MidenProofBundle {
    inputs: Vec<u64>,
    outputs: Vec<u64>,
    proof: Vec<u8>,
}

fn input_to_miden(inputs: &Input) -> anyhow::Result<(StackInputs, AdviceInputs, Vec<u64>)> {
    fn bytes_to_u64_words(bytes: &[u8]) -> Vec<u64> {
        let mut words: Vec<u64> = Vec::with_capacity((bytes.len() + 7) / 8);
        for chunk in bytes.chunks(8) {
            let mut arr = [0u8; 8];
            arr[..chunk.len()].copy_from_slice(chunk);
            words.push(u64::from_le_bytes(arr));
        }
        words
    }

    let mut stack_vals: Vec<u64> = Vec::new();

    for item in inputs.iter() {
        match item {
            InputItem::Bytes(bytes) | InputItem::SerializedObject(bytes) => {
                stack_vals.extend(bytes_to_u64_words(bytes));
            }
            InputItem::Object(obj) => {
                let ser = bincode::serialize(&obj).unwrap_or_default();
                stack_vals.extend(bytes_to_u64_words(&ser));
            }
        }
    }

    let stack_inputs = StackInputs::try_from_ints(stack_vals.clone())
        .map_err(|e| anyhow::anyhow!("stack inputs: {e}"))?;
    Ok((stack_inputs, AdviceInputs::default(), stack_vals))
}
