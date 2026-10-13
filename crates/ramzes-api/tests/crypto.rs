use ramzes_api::SecretKey;
use serde_json::json;

fn key(byte: u8) -> SecretKey {
    use base64::Engine as _;
    SecretKey::from_base64(&base64::engine::general_purpose::STANDARD.encode([byte; 32])).unwrap()
}

#[test]
fn seal_and_open_round_trip() {
    let k = key(1);
    let secret = json!({"webhook_url": "https://hooks.slack.com/services/T/B/x", "n": 1});
    let sealed = k.seal(&secret);
    assert!(!sealed.contains("hooks.slack.com"));
    assert_eq!(k.open(&sealed).unwrap(), secret);
}

#[test]
fn sealing_twice_gives_different_ciphertexts() {
    let k = key(1);
    assert_ne!(k.seal(&json!({"a": 1})), k.seal(&json!({"a": 1})));
}

#[test]
fn a_tampered_ciphertext_does_not_open() {
    use base64::Engine as _;
    let k = key(1);
    let sealed = k.seal(&json!({"a": 1}));
    let mut bytes = base64::engine::general_purpose::STANDARD
        .decode(&sealed)
        .unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    let tampered = base64::engine::general_purpose::STANDARD.encode(bytes);
    assert!(k.open(&tampered).is_err());
    assert!(k.open("not base64 at all!").is_err());
    assert!(k.open("").is_err());
}

#[test]
fn a_different_key_does_not_open() {
    let sealed = key(1).seal(&json!({"a": 1}));
    assert!(key(2).open(&sealed).is_err());
}

#[test]
fn a_key_must_be_32_bytes() {
    use base64::Engine as _;
    assert!(
        SecretKey::from_base64(&base64::engine::general_purpose::STANDARD.encode([0u8; 16]))
            .is_err()
    );
    assert!(SecretKey::from_base64("***").is_err());
}

#[test]
fn load_or_create_makes_the_key_once_and_reads_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let first = SecretKey::load_or_create(dir.path()).unwrap();
    let sealed = first.seal(&json!({"a": 1}));
    let second = SecretKey::load_or_create(dir.path()).unwrap();
    assert_eq!(second.open(&sealed).unwrap(), json!({"a": 1}));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join("secret.key"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
