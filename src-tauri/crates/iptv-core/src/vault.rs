use crate::error::{Error, Result};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use zeroize::Zeroizing;

const MAGIC: &[u8] = b"IPTVKEY1";

/// Sealed storage for source configuration and stream secrets. The 32-byte key is generated
/// once per profile and kept in a private file next to the data, so the application starts
/// without a passphrase. This protects against accidental disclosure (backups, copied files,
/// logs), not against an attacker who already runs as the same user.
pub struct Vault {
    path: PathBuf,
    key: Mutex<Option<Zeroizing<[u8; 32]>>>,
}
impl Vault {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            key: Mutex::new(None),
        }
    }
    pub fn exists(&self) -> bool {
        self.path.exists()
    }
    pub fn is_ready(&self) -> bool {
        self.key.lock().is_ok_and(|key| key.is_some())
    }
    /// Drops the key from memory; used at shutdown.
    pub fn clear(&self) {
        if let Ok(mut key) = self.key.lock() {
            *key = None;
        }
    }
    /// Loads the profile key, creating it on first start. A damaged key file is rejected
    /// rather than silently replaced, because replacing it would orphan every sealed record.
    pub fn initialize(&self) -> Result<()> {
        let mut guard = self.key.lock().map_err(|_| Error::Vault)?;
        if guard.is_some() {
            return Ok(());
        }
        let mut key = Zeroizing::new([0u8; 32]);
        if self.path.exists() {
            let mut bytes = Vec::new();
            fs::File::open(&self.path)?
                .take(MAGIC.len() as u64 + 33)
                .read_to_end(&mut bytes)?;
            if bytes.len() != MAGIC.len() + 32 || !bytes.starts_with(MAGIC) {
                return Err(Error::Vault);
            }
            key.copy_from_slice(&bytes[MAGIC.len()..]);
        } else {
            getrandom::fill(&mut *key).map_err(|_| Error::Vault)?;
            let mut bytes = MAGIC.to_vec();
            bytes.extend_from_slice(&*key);
            write_private_new(&self.path, &bytes)?;
        }
        *guard = Some(key);
        Ok(())
    }
    pub fn seal(&self, context: &str, plain: &[u8]) -> Result<Vec<u8>> {
        let key = self.key.lock().map_err(|_| Error::Vault)?;
        encrypt(key.as_ref().ok_or(Error::Vault)?, context.as_bytes(), plain)
    }
    pub fn open(&self, context: &str, bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let key = self.key.lock().map_err(|_| Error::Vault)?;
        decrypt(key.as_ref().ok_or(Error::Vault)?, context.as_bytes(), bytes)
    }
}
fn encrypt(key: &[u8; 32], aad: &[u8], plain: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| Error::Vault)?;
    let mut nonce = [0u8; 24];
    getrandom::fill(&mut nonce).map_err(|_| Error::Vault)?;
    let mut result = nonce.to_vec();
    result.extend(
        cipher
            .encrypt(&XNonce::from(nonce), Payload { msg: plain, aad })
            .map_err(|_| Error::Vault)?,
    );
    Ok(result)
}
fn decrypt(key: &[u8; 32], aad: &[u8], bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if bytes.len() < 40 {
        return Err(Error::Vault);
    }
    let nonce: [u8; 24] = bytes[..24].try_into().map_err(|_| Error::Vault)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| Error::Vault)?;
    cipher
        .decrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &bytes[24..],
                aad,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| Error::Vault)
}
fn write_private_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = path.with_extension("initializing");
    // Recover only the known incomplete first-creation file, never an existing key.
    if temp.exists() {
        fs::remove_file(&temp)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    if path.exists() {
        return Err(Error::Vault);
    }
    fs::rename(&temp, path)?;
    Ok(())
}
