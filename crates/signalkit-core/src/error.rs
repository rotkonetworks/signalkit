use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Signal Desktop directory not found at {0}")]
    DesktopDirNotFound(PathBuf),

    #[error("config.json has neither 'key' nor 'encryptedKey'")]
    NoKey,

    #[error("unsupported encryptedKey version prefix: {0:?}")]
    UnsupportedKeyVersion([u8; 3]),

    #[error("libsecret has no entry for application={0:?}")]
    LibsecretLookup(String),

    #[error("decryption failed: {0}")]
    Decrypt(String),

    #[error("home directory not found")]
    NoHome,

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    Secret(#[from] secret_service::Error),

    #[error(transparent)]
    Hex(#[from] hex::FromHexError),

    #[error("presage: {0}")]
    Presage(String),

    #[error(transparent)]
    Time(#[from] std::time::SystemTimeError),
}

pub type Result<T> = std::result::Result<T, Error>;
