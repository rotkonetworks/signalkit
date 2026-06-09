use std::collections::HashMap;

use aes::Aes128;
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use secret_service::{EncryptionType, SecretService};
use sha1::Sha1;

use crate::{Error, Result};

const SALT: &[u8] = b"saltysalt";
const IV: &[u8; 16] = b"                ";
const KEY_LEN: usize = 16;
const PASSWORD_V10: &[u8] = b"peanuts";
const ITERATIONS_LINUX: u32 = 1;

type Aes128CbcDec = cbc::Decryptor<Aes128>;

/// Decrypt Signal Desktop's `encryptedKey` from `config.json` into the hex
/// SQLCipher key. Handles Chromium OSCrypt v10 (hardcoded password) and
/// v11 (libsecret-derived password) on Linux.
pub async fn decrypt_signal_db_key(encrypted_hex: &str) -> Result<String> {
    let raw = hex::decode(encrypted_hex)?;
    if raw.len() < 3 {
        return Err(Error::UnsupportedKeyVersion([0, 0, 0]));
    }
    let (prefix, ciphertext) = raw.split_at(3);
    let plaintext = match prefix {
        b"v10" => decrypt_with_password(ciphertext, PASSWORD_V10)?,
        b"v11" => {
            let password = libsecret_password("Signal").await?;
            decrypt_with_password(ciphertext, password.as_bytes())?
        }
        other => {
            let mut p = [0u8; 3];
            p.copy_from_slice(other);
            return Err(Error::UnsupportedKeyVersion(p));
        }
    };
    String::from_utf8(plaintext).map_err(|e| Error::Decrypt(e.to_string()))
}

fn decrypt_with_password(ciphertext: &[u8], password: &[u8]) -> Result<Vec<u8>> {
    let mut key = [0u8; KEY_LEN];
    pbkdf2::pbkdf2_hmac::<Sha1>(password, SALT, ITERATIONS_LINUX, &mut key);
    let mut buf = ciphertext.to_vec();
    let pt_len = Aes128CbcDec::new(&key.into(), IV.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| Error::Decrypt(e.to_string()))?
        .len();
    buf.truncate(pt_len);
    Ok(buf)
}

async fn libsecret_password(application: &str) -> Result<String> {
    let ss = SecretService::connect(EncryptionType::Dh).await?;
    let mut attrs: HashMap<&str, &str> = HashMap::new();
    attrs.insert("application", application);
    let items = ss.search_items(attrs).await?;
    let item = items
        .unlocked
        .into_iter()
        .next()
        .or_else(|| items.locked.into_iter().next())
        .ok_or_else(|| Error::LibsecretLookup(application.to_string()))?;
    if item.is_locked().await? {
        item.unlock().await?;
    }
    let secret = item.get_secret().await?;
    String::from_utf8(secret).map_err(|e| Error::Decrypt(e.to_string()))
}
