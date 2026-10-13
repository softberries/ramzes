use std::io::{self, Write};
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use ramzes_engine::Opener;
use sha2::{Digest, Sha256};

const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 24;
const KEY_FILE: &str = "secret.key";

/// The instance key that encrypts connection credentials at rest (XChaCha20-Poly1305).
pub struct SecretKey {
    cipher: XChaCha20Poly1305,
    /// Derived from the key; signs links such as the one that acknowledges a notification.
    signing: [u8; 32],
}

impl SecretKey {
    fn from_bytes(bytes: &[u8]) -> Result<SecretKey, String> {
        let key: [u8; KEY_BYTES] = bytes.try_into().map_err(|_| {
            format!(
                "the secret key must be {KEY_BYTES} bytes, got {}",
                bytes.len()
            )
        })?;
        let mut derive = Sha256::new();
        derive.update(b"ramzes link signing v1");
        derive.update(key);
        Ok(SecretKey {
            cipher: XChaCha20Poly1305::new(&key.into()),
            signing: derive.finalize().into(),
        })
    }

    /// A key given as base64 of 32 bytes, for example from `openssl rand -base64 32`.
    pub fn from_base64(text: &str) -> Result<SecretKey, String> {
        let bytes = STANDARD
            .decode(text.trim())
            .map_err(|e| format!("the secret key is not valid base64: {e}"))?;
        SecretKey::from_bytes(&bytes)
    }

    /// Reads `secret.key` from the data directory, creating it with a random key on first use.
    pub fn load_or_create(data_dir: &Path) -> io::Result<SecretKey> {
        let path = data_dir.join(KEY_FILE);
        let invalid = |e: String| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {e}", path.display()),
            )
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => SecretKey::from_base64(&text).map_err(invalid),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let bytes: [u8; KEY_BYTES] = rand::random();
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options.open(&path)?;
                file.write_all(STANDARD.encode(bytes).as_bytes())?;
                file.sync_all()?;
                SecretKey::from_bytes(&bytes).map_err(invalid)
            }
            Err(e) => Err(e),
        }
    }

    /// Encrypts a JSON value. The result is base64 of a random nonce followed by the ciphertext.
    pub fn seal(&self, value: &serde_json::Value) -> String {
        let nonce: [u8; NONCE_BYTES] = rand::random();
        let ciphertext = self
            .cipher
            .encrypt(&XNonce::from(nonce), value.to_string().as_bytes())
            .expect("encrypting an in-memory buffer cannot fail");
        let mut out = nonce.to_vec();
        out.extend(ciphertext);
        STANDARD.encode(out)
    }

    pub fn open(&self, sealed: &str) -> Result<serde_json::Value, String> {
        let bytes = STANDARD
            .decode(sealed)
            .map_err(|_| "not valid base64".to_owned())?;
        if bytes.len() <= NONCE_BYTES {
            return Err("too short".to_owned());
        }
        let (nonce, ciphertext) = bytes.split_at(NONCE_BYTES);
        let nonce: [u8; NONCE_BYTES] = nonce.try_into().map_err(|_| "too short".to_owned())?;
        let plain = self
            .cipher
            .decrypt(&XNonce::from(nonce), ciphertext)
            .map_err(|_| {
                "decryption failed; was it sealed with a different secret key?".to_owned()
            })?;
        serde_json::from_slice(&plain).map_err(|e| e.to_string())
    }
}

impl Opener for SecretKey {
    fn open(&self, sealed: &str) -> Result<serde_json::Value, String> {
        SecretKey::open(self, sealed)
    }

    fn sign(&self, text: &str) -> String {
        let mut mac = <Hmac<Sha256> as hmac::KeyInit>::new_from_slice(&self.signing)
            .expect("HMAC accepts a key of any length");
        mac.update(text.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }
}
