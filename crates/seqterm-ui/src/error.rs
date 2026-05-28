/// Centralized error type for the SeqTerm application layer.
///
/// All fallible `AppCommand` handlers should return or report through this type
/// so errors can be surfaced as `Modal::error` dialogs rather than panics.
#[derive(Debug)]
pub enum AppError {
    Io(std::io::Error),
    Midi(String),
    Persistence(String),
    ThreadSpawn(std::io::Error),
    InvalidInput(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Io(e)           => write!(f, "I/O error: {e}"),
            AppError::Midi(s)         => write!(f, "MIDI error: {s}"),
            AppError::Persistence(s)  => write!(f, "Project error: {s}"),
            AppError::ThreadSpawn(e)  => write!(f, "Thread error: {e}"),
            AppError::InvalidInput(s) => write!(f, "Invalid input: {s}"),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self { AppError::Io(e) }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self { AppError::Persistence(e.to_string()) }
}
