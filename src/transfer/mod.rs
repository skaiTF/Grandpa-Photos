//! Everything that talks to the phone and moves files. Nothing in here knows about the UI.

pub mod fake;
pub mod source;
pub mod worker;

/// Errors that can happen while reading from the phone or writing to disk.
///
/// More variants (e.g. `InsufficientDiskSpace`) are added in later milestones as the
/// code that produces them is written; unused variants would fail `clippy -D warnings`.
#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("the file `{0}` was not found on the phone")]
    NotFound(String),
    #[error("input/output error: {0}")]
    Io(#[from] std::io::Error),
}
