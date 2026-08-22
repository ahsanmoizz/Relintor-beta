use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_standards::{builtin_registry, sign_production_registry_artifact};
use std::env;

fn main() {
    let encoded = env::var("RELINTOR_REGISTRY_SIGNING_KEY_B64")
        .expect("RELINTOR_REGISTRY_SIGNING_KEY_B64 must be supplied by release tooling");
    let bytes = BASE64
        .decode(encoded)
        .expect("RELINTOR_REGISTRY_SIGNING_KEY_B64 must be valid base64");
    let key_bytes: [u8; 32] = bytes
        .try_into()
        .expect("RELINTOR_REGISTRY_SIGNING_KEY_B64 must decode to 32 bytes");
    let signer_key_id = env::var("RELINTOR_REGISTRY_SIGNER_KEY_ID")
        .expect("RELINTOR_REGISTRY_SIGNER_KEY_ID must be supplied by release tooling");
    let artifact = sign_production_registry_artifact(
        &builtin_registry(),
        &signer_key_id,
        &SigningKey::from_bytes(&key_bytes),
    )
    .expect("sign production registry artifact");
    println!(
        "{}",
        serde_json::to_string_pretty(&artifact).expect("serialize artifact")
    );
}
