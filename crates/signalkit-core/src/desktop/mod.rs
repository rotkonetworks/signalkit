pub mod db;
pub mod keychain;

use std::path::PathBuf;

use serde::Deserialize;

use crate::{Error, Result};
use db::DesktopDb;

#[derive(Deserialize)]
struct ConfigJson {
    key: Option<String>,
    #[serde(rename = "encryptedKey")]
    encrypted_key: Option<String>,
}

pub struct DesktopBundle {
    pub root: PathBuf,
    pub db: DesktopDb,
}

impl DesktopBundle {
    pub async fn open(root: PathBuf) -> Result<Self> {
        if !root.exists() {
            return Err(Error::DesktopDirNotFound(root));
        }
        let config_path = root.join("config.json");
        let config_bytes = tokio::fs::read(&config_path).await?;
        let config: ConfigJson = serde_json::from_slice(&config_bytes)?;

        let hex_key = match (config.key, config.encrypted_key) {
            (Some(k), _) => k,
            (None, Some(enc)) => keychain::decrypt_signal_db_key(&enc).await?,
            (None, None) => return Err(Error::NoKey),
        };

        let db_path = root.join("sql/db.sqlite");
        let db = DesktopDb::open(&db_path, &hex_key)?;
        Ok(Self { root, db })
    }
}
