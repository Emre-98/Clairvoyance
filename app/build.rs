fn main() {
    // Run on the gaming GPU of hybrid laptops (see crates/cv-capture/src/win/d3d.rs).
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/EXPORT:NvOptimusEnablement,DATA");
        println!("cargo:rustc-link-arg-bins=/EXPORT:AmdPowerXpressRequestHighPerformance,DATA");
    }
    seal_config();
    tauri_build::build()
}

/// The tamper seal's settings (see crates/cv-seal and src/integrity.rs): `CV_SEAL_REQUIRED=1`
/// (Release workflow) makes the app refuse to start unless it's sealed with the key whose public
/// half is `CV_SEAL_PUBKEY` (64 hex). The public key is stored masked, so a search of the
/// executable doesn't find it. Without them (dev and test builds) the check is off.
fn seal_config() {
    println!("cargo:rerun-if-env-changed=CV_SEAL_REQUIRED");
    println!("cargo:rerun-if-env-changed=CV_SEAL_PUBKEY");
    let required = std::env::var("CV_SEAL_REQUIRED").as_deref() == Ok("1");
    let key = std::env::var("CV_SEAL_PUBKEY").unwrap_or_default();
    let key = key.trim();
    let public: [u8; 32] = if key.is_empty() {
        assert!(!required, "CV_SEAL_REQUIRED=1 needs CV_SEAL_PUBKEY (the seal's public key, 64 hex characters)");
        [0; 32]
    } else {
        let bytes: Vec<u8> = (0..key.len())
            .step_by(2)
            .map(|i| key.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).expect("CV_SEAL_PUBKEY must be hex"))
            .collect();
        bytes.try_into().expect("CV_SEAL_PUBKEY must be 32 bytes (64 hex characters)")
    };
    // A per-version mask (any bytes do: it only keeps the key out of a plain search).
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (env!("CARGO_PKG_VERSION"), key).hash(&mut h);
    let mut x = h.finish() | 1;
    let mask: Vec<u8> = (0..32)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    let masked: Vec<u8> = public.iter().zip(&mask).map(|(p, m)| p ^ m).collect();
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("seal_config.rs");
    std::fs::write(out, format!("pub const SEAL_REQUIRED: bool = {required};\npub const SEAL_PUBKEY_MASKED: [u8; 32] = {masked:?};\npub const SEAL_PUBKEY_MASK: [u8; 32] = {mask:?};\n")).unwrap();
}
