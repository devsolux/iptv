use std::{fs::{self, OpenOptions}, io::{Read, Write}, path::{Path, PathBuf}, sync::Mutex};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{aead::{Aead, KeyInit, Payload}, XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;
use crate::error::{Error, Result};

const MAGIC: &[u8] = b"IPTVVAULT1";
const CHECK: &[u8] = b"IPTV 1.0 local vault verification";
const AAD: &[u8] = b"com.devsolux.iptv/vault/v1";

pub struct Vault { path: PathBuf, key: Mutex<Option<Zeroizing<[u8; 32]>>> }
impl Vault {
    pub fn new(path: PathBuf) -> Self { Self { path, key: Mutex::new(None) } }
    pub fn exists(&self) -> bool { self.path.exists() }
    pub fn is_unlocked(&self) -> bool { self.key.lock().is_ok_and(|key| key.is_some()) }
    pub fn lock(&self) { if let Ok(mut key) = self.key.lock() { *key = None; } }

    pub fn unlock(&self, password: Zeroizing<String>) -> Result<()> {
        if password.len() > 1024 { return Err(Error::Limit); }
        let mut guard = self.key.lock().map_err(|_| Error::Vault)?;
        let (salt, existing) = if self.path.exists() {
            let mut bytes = Vec::new();
            fs::File::open(&self.path)?.take(4097).read_to_end(&mut bytes)?;
            if bytes.len() > 4096 || bytes.len() < MAGIC.len() + 16 + 40 || !bytes.starts_with(MAGIC) {
                return Err(Error::Vault);
            }
            let mut salt = [0u8; 16];
            salt.copy_from_slice(&bytes[MAGIC.len()..MAGIC.len()+16]);
            (salt, Some(bytes[MAGIC.len()+16..].to_vec()))
        } else {
            if password.chars().count() < 12 { return Err(Error::Invalid("Use a passphrase of at least 12 characters.")); }
            let mut salt = [0u8; 16]; getrandom::fill(&mut salt).map_err(|_| Error::Vault)?;
            (salt, None)
        };
        // Fixed, versioned KDF parameters: 32 MiB, three passes, one lane.
        let params = Params::new(32 * 1024, 3, 1, Some(32)).map_err(|_| Error::Vault)?;
        let kdf = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut key = Zeroizing::new([0u8; 32]);
        kdf.hash_password_into(password.as_bytes(), &salt, &mut *key).map_err(|_| Error::Vault)?;
        if let Some(bytes) = existing {
            let plain = decrypt(&key, AAD, &bytes)?;
            if plain.as_slice() != CHECK { return Err(Error::Vault); }
        } else {
            let mut bytes = MAGIC.to_vec(); bytes.extend_from_slice(&salt);
            bytes.extend_from_slice(&encrypt(&key, AAD, CHECK)?);
            write_private_new(&self.path, &bytes)?;
        }
        *guard = Some(key);
        Ok(())
    }
    pub fn seal(&self, context: &str, plain: &[u8]) -> Result<Vec<u8>> {
        let key = self.key.lock().map_err(|_| Error::Vault)?;
        encrypt(key.as_ref().ok_or(Error::Locked)?, context.as_bytes(), plain)
    }
    pub fn open(&self, context: &str, bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let key = self.key.lock().map_err(|_| Error::Vault)?;
        decrypt(key.as_ref().ok_or(Error::Locked)?, context.as_bytes(), bytes)
    }
}
fn encrypt(key: &[u8;32], aad: &[u8], plain: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| Error::Vault)?;
    let mut nonce = [0u8;24]; getrandom::fill(&mut nonce).map_err(|_| Error::Vault)?;
    let mut result = nonce.to_vec();
    result.extend(cipher.encrypt(&XNonce::from(nonce), Payload { msg: plain, aad }).map_err(|_| Error::Vault)?);
    Ok(result)
}
fn decrypt(key: &[u8;32], aad: &[u8], bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if bytes.len() < 40 { return Err(Error::Vault); }
    let nonce: [u8;24] = bytes[..24].try_into().map_err(|_| Error::Vault)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| Error::Vault)?;
    cipher.decrypt(&XNonce::from(nonce), Payload { msg: &bytes[24..], aad })
        .map(Zeroizing::new).map_err(|_| Error::Vault)
}
fn write_private_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = path.with_extension("initializing");
    // Recover only the known incomplete first-creation file, never an existing vault.
    if temp.exists() { fs::remove_file(&temp)?; }
    let mut options = OpenOptions::new(); options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let mut file = options.open(&temp)?; file.write_all(bytes)?; file.sync_all()?;
    if path.exists() { return Err(Error::Vault); }
    fs::rename(&temp, path)?;
    Ok(())
}
