//! Live Signal protocol via presage: link as a secondary device, send, receive.
//!
//! The presage `Manager` is `!Send`, so all calls here must be made from a
//! single thread — typically a `tokio::task::LocalSet`. Callers (CLI / Tauri
//! actor) own that LocalSet; this module is just async free functions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use futures::channel::oneshot;
use futures::{future, pin_mut, StreamExt};
use presage::libsignal_service::configuration::SignalServers;
use presage::libsignal_service::content::{ContentBody, DataMessage};
use presage::libsignal_service::protocol::ServiceId;
use presage::model::identity::OnNewIdentity;
use presage::model::messages::Received;
use presage::Manager;
use presage_store_sqlite::SqliteStore;
use rand::RngCore;
use secret_service::{EncryptionType, SecretService};
use uuid::Uuid;

use crate::{Error, Result};

pub use url::Url;

/// `~/.local/share/signalkit/presage.sqlite3`
pub fn default_store_path() -> Result<PathBuf> {
    let base = dirs::data_dir().ok_or(Error::NoHome)?;
    Ok(base.join("signalkit").join("presage.sqlite3"))
}

pub async fn open_store(path: &Path) -> Result<SqliteStore> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
        let _ = tokio::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).await;
    }

    let passphrase = get_or_create_store_passphrase().await?;

    // Migrate a pre-existing plain store to SQLCipher-encrypted, transparently.
    {
        let path_owned = path.to_path_buf();
        let pp_owned = passphrase.clone();
        tokio::task::spawn_blocking(move || migrate_to_encrypted(&path_owned, &pp_owned))
            .await
            .map_err(|e| Error::Presage(format!("migration join: {e}")))??;
    }

    let path_str = path.to_string_lossy().to_string();
    let store =
        SqliteStore::open_with_passphrase(&path_str, Some(&passphrase), OnNewIdentity::Trust)
            .await
            .map_err(|e| Error::Presage(format!("open store: {e}")))?;
    if tokio::fs::try_exists(path).await.unwrap_or(false) {
        let _ = tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await;
    }
    Ok(store)
}

/// Get the presage store passphrase from libsecret, generating + storing a new
/// random 32-byte hex one if no entry exists yet.
async fn get_or_create_store_passphrase() -> Result<String> {
    let ss = SecretService::connect(EncryptionType::Dh).await?;

    let mut attrs: HashMap<&str, &str> = HashMap::new();
    attrs.insert("application", "signalkit");
    attrs.insert("purpose", "presage-store");

    let found = ss.search_items(attrs.clone()).await?;
    let existing = found
        .unlocked
        .into_iter()
        .next()
        .or_else(|| found.locked.into_iter().next());
    if let Some(item) = existing {
        if item.is_locked().await? {
            item.unlock().await?;
        }
        let secret = item.get_secret().await?;
        return String::from_utf8(secret).map_err(|e| Error::Decrypt(e.to_string()));
    }

    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let passphrase = hex::encode(bytes);

    let collection = ss.get_default_collection().await?;
    if collection.is_locked().await? {
        collection.unlock().await?;
    }
    collection
        .create_item(
            "signalkit presage store",
            attrs,
            passphrase.as_bytes(),
            true,
            "text/plain",
        )
        .await?;

    Ok(passphrase)
}

/// If the file at `path` is a plaintext SQLite DB, copy it through
/// `sqlcipher_export` into an encrypted file using `passphrase`, then atomically
/// swap. Existing already-encrypted-with-this-key DBs are left alone. Any other
/// state (encrypted with a different key, or corrupt) is left untouched and an
/// error is returned.
fn migrate_to_encrypted(path: &Path, passphrase: &str) -> Result<()> {
    use rusqlite::Connection;

    if !path.exists() {
        return Ok(());
    }

    // 1. Already encrypted with this key?
    {
        let conn = Connection::open(path)?;
        let pp_esc = passphrase.replace('\'', "''");
        let _ = conn.pragma_update(None, "key", &pp_esc);
        if conn
            .query_row("SELECT count(*) FROM sqlite_master", [], |row| {
                row.get::<_, i64>(0)
            })
            .is_ok()
        {
            return Ok(());
        }
    }

    // 2. Openable as plain (no key)?
    let conn = Connection::open(path)?;
    if conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })
        .is_err()
    {
        return Err(Error::Presage(format!(
            "presage store at {} is neither plain nor encrypted with the libsecret key — refusing to overwrite. Inspect it manually.",
            path.display()
        )));
    }

    // 3. Export through SQLCipher into a new encrypted file alongside.
    let new_path = path.with_file_name(format!(
        "{}.enc.new",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("presage")
    ));
    let _ = std::fs::remove_file(&new_path);

    let pp_esc = passphrase.replace('\'', "''");
    let new_path_str = new_path.to_string_lossy().replace('\'', "''");
    conn.execute_batch(&format!(
        "ATTACH DATABASE '{new_path_str}' AS encrypted KEY '{pp_esc}';
         SELECT sqlcipher_export('encrypted');
         DETACH DATABASE encrypted;"
    ))?;
    drop(conn);

    // 4. Swap with a .plain.bak fallback.
    let backup_path = path.with_file_name(format!(
        "{}.plain.bak",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("presage")
    ));
    std::fs::rename(path, &backup_path)?;
    if let Err(e) = std::fs::rename(&new_path, path) {
        // Restore on failure.
        let _ = std::fs::rename(&backup_path, path);
        return Err(Error::Io(e));
    }

    // 5. WAL/SHM files belong to the old plain DB; remove them so SQLCipher
    //    doesn't try to use them.
    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
        let _ = std::fs::remove_file(path.with_file_name(format!("{name}-shm")));
        let _ = std::fs::remove_file(path.with_file_name(format!("{name}-wal")));
    }

    tracing::warn!(
        target: "signalkit",
        "migrated presage store at {} to encrypted-at-rest. Plain-text backup kept at {}; delete it after verifying.",
        path.display(),
        backup_path.display()
    );

    Ok(())
}

/// Link this app as a secondary Signal device. Calls `on_url` with the
/// `sgnl://linkdevice?...` URL to display as a QR code; awaits phone scan.
pub async fn link<F>(store: SqliteStore, device_name: String, on_url: F) -> Result<()>
where
    F: FnOnce(Url) + 'static,
{
    let (tx, rx) = oneshot::channel();
    let (mgr_res, _) = future::join(
        Manager::link_secondary_device(store, SignalServers::Production, device_name, tx),
        async move {
            if let Ok(url) = rx.await {
                on_url(url);
            }
        },
    )
    .await;
    mgr_res
        .map(|_| ())
        .map_err(|e| Error::Presage(format!("link: {e}")))
}

/// Load an already-linked device. Returns Err if this app has never been linked.
pub async fn load_registered(store: SqliteStore) -> Result<Manager<SqliteStore, presage::manager::Registered>> {
    Manager::load_registered(store)
        .await
        .map_err(|e| Error::Presage(format!("load: {e}")))
}

#[derive(Debug, serde::Serialize)]
pub struct WhoAmI {
    pub aci: Uuid,
    pub pni: Option<Uuid>,
    pub number: Option<String>,
}

pub async fn whoami(
    manager: &mut Manager<SqliteStore, presage::manager::Registered>,
) -> Result<WhoAmI> {
    let r = manager
        .whoami()
        .await
        .map_err(|e| Error::Presage(format!("whoami: {e}")))?;
    let dbg = format!("{r:?}");
    let aci = extract_uuid(&dbg, &["aci:", "uuid:"])
        .ok_or_else(|| Error::Presage(format!("no aci in whoami response: {dbg}")))?;
    let pni = extract_uuid(&dbg, &["pni:"]);
    let number = extract_string(&dbg, &["number:", "e164:"]);
    Ok(WhoAmI { aci, pni, number })
}

fn extract_uuid(s: &str, keys: &[&str]) -> Option<Uuid> {
    let raw = extract_field(s, keys)?;
    let trimmed = raw.trim_matches(|c: char| !c.is_ascii_hexdigit() && c != '-');
    Uuid::parse_str(trimmed).ok()
}

fn extract_string(s: &str, keys: &[&str]) -> Option<String> {
    let raw = extract_field(s, keys)?;
    Some(raw.trim_matches(|c: char| c == '"' || c == ',' || c == '}' || c.is_whitespace()).to_string())
}

fn extract_field(s: &str, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(idx) = s.find(k) {
            let after = &s[idx + k.len()..];
            let end = after.find(|c: char| c == ',' || c == '}').unwrap_or(after.len());
            return Some(after[..end].trim().to_string());
        }
    }
    None
}

/// Send a 1:1 text message to a Signal user identified by their ACI UUID.
pub async fn send_text(
    manager: &mut Manager<SqliteStore, presage::manager::Registered>,
    recipient: Uuid,
    body: String,
) -> Result<u64> {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    let content = ContentBody::DataMessage(DataMessage {
        body: Some(body),
        timestamp: Some(ts),
        ..Default::default()
    });
    manager
        .send_message(ServiceId::Aci(recipient.into()), content, ts)
        .await
        .map_err(|e| Error::Presage(format!("send: {e}")))?;
    Ok(ts)
}

/// Send a text message to a Signal group v2, identified by its 32-byte master key.
pub async fn send_text_to_group(
    manager: &mut Manager<SqliteStore, presage::manager::Registered>,
    master_key: &[u8; 32],
    body: String,
) -> Result<u64> {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    let content = ContentBody::DataMessage(DataMessage {
        body: Some(body),
        timestamp: Some(ts),
        ..Default::default()
    });
    manager
        .send_message_to_group(master_key, content, ts)
        .await
        .map_err(|e| Error::Presage(format!("send_group: {e}")))?;
    Ok(ts)
}

/// Decode a base64-encoded 32-byte group master key.
pub fn decode_master_key(b64: &str) -> Result<[u8; 32]> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    let raw = STANDARD
        .decode(b64.trim())
        .map_err(|e| Error::Presage(format!("master key base64: {e}")))?;
    let len = raw.len();
    <[u8; 32]>::try_from(raw)
        .map_err(|_| Error::Presage(format!("master key must be 32 bytes, got {len}")))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IncomingMessage {
    pub from: String,
    pub body: String,
    pub timestamp: u64,
    pub has_attachments: bool,
}

/// Run the receive loop, invoking `on_msg` for each incoming text message.
/// Returns when the stream closes (manager is dropped or unrecoverable error).
pub async fn receive_into<F>(
    manager: &mut Manager<SqliteStore, presage::manager::Registered>,
    mut on_msg: F,
) -> Result<()>
where
    F: FnMut(IncomingMessage),
{
    let stream = manager
        .receive_messages()
        .await
        .map_err(|e| Error::Presage(format!("receive_messages: {e}")))?;
    pin_mut!(stream);
    while let Some(item) = stream.next().await {
        if let Received::Content(content) = item {
            let body_opt = match &content.body {
                ContentBody::DataMessage(dm) => dm.body.clone(),
                ContentBody::SynchronizeMessage(sm) => sm
                    .sent
                    .as_ref()
                    .and_then(|s| s.message.as_ref())
                    .and_then(|m| m.body.clone()),
                _ => None,
            };
            if let Some(body) = body_opt {
                on_msg(IncomingMessage {
                    from: format!("{:?}", content.metadata.sender),
                    body,
                    timestamp: content.metadata.timestamp,
                    has_attachments: matches!(&content.body, ContentBody::DataMessage(dm) if !dm.attachments.is_empty()),
                });
            }
        }
    }
    Ok(())
}
