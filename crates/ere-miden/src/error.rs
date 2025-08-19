use thiserror::Error;

#[derive(Debug, Error)]
pub enum CompileError {
    #[error("Invalid program source: {0}")]
    InvalidSource(String),
}

#[derive(Debug, Error)]
pub enum ExecuteError {
    #[error("Execution failed: {0}")]
    Client(String),
}

#[derive(Debug, Error)]
pub enum ProveError {
    #[error("Proving failed: {0}")]
    Client(String),
}

#[derive(Debug, Error)]
pub enum VerifyError {
    #[error("Verify failed: {0}")]
    Client(String),
}

#[derive(Debug, Error)]
pub enum MidenError {
    #[error(transparent)]
    Compile(#[from] CompileError),
    #[error(transparent)]
    Execute(#[from] ExecuteError),
    #[error(transparent)]
    Prove(#[from] ProveError),
    #[error(transparent)]
    Verify(#[from] VerifyError),
}
