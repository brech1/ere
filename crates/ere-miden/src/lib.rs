//! Ere Miden zkVM interface.

pub mod compile;
pub mod error;
pub mod input;

use self::error::{ExecuteError, MidenError, VerifyError};
use self::input::generate_miden_inputs;
use miden_core::{
    Program,
    utils::{Deserializable, Serializable},
};
use miden_processor::{
    DefaultHost, ExecutionOptions, ProgramInfo, StackInputs, StackOutputs, execute as miden_execute,
};
use miden_prover::{ExecutionProof, ProvingOptions, prove as miden_prove};
use miden_stdlib::StdLibrary;
use miden_verifier::verify as miden_verify;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{env, io::Read, time::Instant};
use zkvm_interface::{
    Input, ProgramExecutionReport, ProgramProvingReport, Proof, PublicValues, zkVM, zkVMError,
};

include!(concat!(env!("OUT_DIR"), "/name_and_sdk_version.rs"));

#[derive(Clone, Serialize, Deserialize)]
pub struct MidenProgram {
    pub program_bytes: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct MidenProofBundle {
    stack_inputs: Vec<u8>,
    stack_outputs: Vec<u8>,
    proof: Vec<u8>,
}

pub struct EreMiden {
    program: Program,
}

fn outputs_to_public_values(outputs: &StackOutputs) -> Result<PublicValues, bincode::Error> {
    let output_ints: Vec<u64> = outputs.iter().map(|f| f.as_int()).collect();
    bincode::serialize(&output_ints)
}

impl EreMiden {
    /// Creates a new `EreMiden` instance from compiled program bytes.
    pub fn new(program: MidenProgram) -> Result<Self, MidenError> {
        let program = Program::read_from_bytes(&program.program_bytes)
            .map_err(ExecuteError::ProgramDeserialization)
            .map_err(MidenError::Execute)?;
        Ok(Self { program })
    }

    fn setup_host() -> Result<DefaultHost, MidenError> {
        let mut host = DefaultHost::default();

        host.load_library(&StdLibrary::default())
            .map_err(ExecuteError::Execution)
            .map_err(MidenError::Execute)?;
        Ok(host)
    }
}

impl zkVM for EreMiden {
    fn execute(&self, inputs: &Input) -> Result<(PublicValues, ProgramExecutionReport), zkVMError> {
        let (stack_inputs, advice_inputs) = generate_miden_inputs(inputs)?;
        let mut host = Self::setup_host()?;

        let start = Instant::now();
        let trace = miden_execute(
            &self.program,
            stack_inputs,
            advice_inputs,
            &mut host,
            ExecutionOptions::default(),
        )
        .map_err(|e| MidenError::Execute(e.into()))?;

        let public_values = outputs_to_public_values(trace.stack_outputs())
            .map_err(|e| MidenError::Execute(e.into()))?;

        let report = ProgramExecutionReport {
            total_num_cycles: trace.trace_len_summary().main_trace_len() as u64,
            execution_duration: start.elapsed(),
            ..Default::default()
        };

        Ok((public_values, report))
    }

    fn prove(
        &self,
        inputs: &Input,
    ) -> Result<(PublicValues, Proof, ProgramProvingReport), zkVMError> {
        let (stack_inputs, advice_inputs) = generate_miden_inputs(inputs)?;
        let mut host = Self::setup_host()?;

        let start = Instant::now();
        let proving_options = ProvingOptions::with_96_bit_security(env::var("MIDEN_DEBUG").is_ok());

        let (stack_outputs, proof) = miden_prove(
            &self.program,
            stack_inputs.clone(),
            advice_inputs,
            &mut host,
            proving_options,
        )
        .map_err(|e| MidenError::Prove(e.into()))?;

        let public_values =
            outputs_to_public_values(&stack_outputs).map_err(|e| MidenError::Prove(e.into()))?;

        let bundle = MidenProofBundle {
            stack_inputs: stack_inputs.to_bytes(),
            stack_outputs: stack_outputs.to_bytes(),
            proof: proof.to_bytes(),
        };

        let proof_bytes = bincode::serialize(&bundle).map_err(|e| MidenError::Prove(e.into()))?;

        Ok((
            public_values,
            proof_bytes,
            ProgramProvingReport::new(start.elapsed()),
        ))
    }

    fn verify(&self, proof: &[u8]) -> Result<PublicValues, zkVMError> {
        let bundle: MidenProofBundle = bincode::deserialize(proof)
            .map_err(|e| MidenError::Verify(VerifyError::BundleDeserialization(e)))?;

        let program_info: ProgramInfo = self.program.clone().into();

        let stack_inputs = StackInputs::read_from_bytes(&bundle.stack_inputs)
            .map_err(|e| MidenError::Verify(VerifyError::MidenDeserialization(e)))?;
        let stack_outputs = StackOutputs::read_from_bytes(&bundle.stack_outputs)
            .map_err(|e| MidenError::Verify(VerifyError::MidenDeserialization(e)))?;
        let execution_proof = ExecutionProof::from_bytes(&bundle.proof)
            .map_err(|e| MidenError::Verify(VerifyError::MidenDeserialization(e)))?;

        miden_verify(
            program_info,
            stack_inputs,
            stack_outputs.clone(),
            execution_proof,
        )
        .map_err(|e| MidenError::Verify(e.into()))?;

        Ok(outputs_to_public_values(&stack_outputs)
            .map_err(|e| MidenError::Verify(VerifyError::BundleDeserialization(e)))?)
    }

    fn name(&self) -> &'static str {
        NAME
    }

    fn sdk_version(&self) -> &'static str {
        SDK_VERSION
    }

    fn deserialize_from<R: Read, T: DeserializeOwned>(&self, reader: R) -> Result<T, zkVMError> {
        bincode::deserialize_from(reader).map_err(|e| MidenError::Execute(e.into()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::MIDEN_TARGET;
    use std::path::PathBuf;
    use test_utils::host::{BasicProgramIo, run_zkvm_execute, run_zkvm_prove};
    use zkvm_interface::Compiler;

    fn load_miden_program(guest_name: &str) -> MidenProgram {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let guest_dir = PathBuf::from(manifest_dir)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(format!("tests/miden/{guest_name}"));

        MIDEN_TARGET.compile(&guest_dir).unwrap()
    }

    #[test]
    fn test_execute_valid_inputs() {
        let guest_programs = ["basic", "fib"];
        let io = BasicProgramIo::valid();

        for guest_name in guest_programs {
            let program = load_miden_program(guest_name);
            let zkvm = EreMiden::new(program).unwrap();
            run_zkvm_execute(&zkvm, &io);
        }
    }

    #[test]
    fn test_prove_valid_inputs() {
        let guest_programs = ["basic", "fib"];
        let io = BasicProgramIo::valid();

        for guest_name in guest_programs {
            let program = load_miden_program(guest_name);
            let zkvm = EreMiden::new(program).unwrap();
            run_zkvm_prove(&zkvm, &io);
        }
    }

    #[test]
    fn test_prove_invalid_inputs() {
        let guest_programs = ["basic", "fib"];
        let invalid_inputs = [
            BasicProgramIo::empty(),
            BasicProgramIo::invalid_type(),
            BasicProgramIo::invalid_data(),
        ];

        for guest_name in guest_programs {
            let program = load_miden_program(guest_name);
            let zkvm = EreMiden::new(program).unwrap();

            for inputs in &invalid_inputs {
                assert!(
                    zkvm.prove(inputs).is_err(),
                    "Proving should fail for guest '{}' with invalid inputs",
                    guest_name
                );
            }
        }
    }
}
