use thiserror::Error;

#[derive(Debug, Error)]
pub enum StzError {
    #[error("invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("unsupported format version: {0}")]
    UnsupportedVersion(u32),

    #[error("corrupt archive: {0}")]
    CorruptArchive(String),

    #[error("missing asset: {0}")]
    MissingAsset(String),

    #[error("missing object: {0}")]
    MissingObject(String),

    #[error("invalid routing graph: {0}")]
    InvalidRoutingGraph(String),

    #[error("registry mismatch: {0}")]
    RegistryMismatch(String),

    #[error("migration failed v{from} → v{to}: {reason}")]
    MigrationFailed { from: u32, to: u32, reason: String },

    #[error("asset integrity failure {uuid}: {reason}")]
    AssetIntegrityFailure { uuid: String, reason: String },

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
}

pub type StzResult<T> = Result<T, StzError>;
