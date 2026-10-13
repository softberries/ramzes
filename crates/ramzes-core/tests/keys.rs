use ramzes_core::{generate_key, hash_key};

#[test]
fn hashing_is_stable_sha256_hex() {
    assert_eq!(
        hash_key("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_ne!(hash_key("abc"), hash_key("abd"));
}

#[test]
fn generated_keys_carry_the_prefix_and_are_unique() {
    let (key, display) = generate_key("rz_src_");
    assert!(key.starts_with("rz_src_"));
    assert!(key.len() >= "rz_src_".len() + 40, "{key}");
    assert!(
        key["rz_src_".len()..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    );
    assert_eq!(display, key[.."rz_src_".len() + 4]);
    assert_ne!(generate_key("rz_src_").0, key);
}
