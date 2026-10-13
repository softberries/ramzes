use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

/// Characters of the secret part shown next to a key so people can tell keys apart.
const DISPLAY_CHARS: usize = 4;

/// The form in which keys and tokens are stored: SHA-256 as lowercase hex.
pub fn hash_key(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

/// A new random key with 256 bits of entropy, and the short prefix that may be displayed.
pub fn generate_key(prefix: &str) -> (String, String) {
    let secret: [u8; 32] = rand::random();
    let key = format!("{prefix}{}", URL_SAFE_NO_PAD.encode(secret));
    let display = key[..prefix.len() + DISPLAY_CHARS].to_owned();
    (key, display)
}
