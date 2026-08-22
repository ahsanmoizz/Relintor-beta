fn main() {
    println!("cargo:rerun-if-env-changed=RELINTOR_CLOUD_API_ENDPOINT");
    println!("cargo:rerun-if-env-changed=RELINTOR_GOOGLE_CLIENT_ID");
    println!("cargo:rerun-if-env-changed=RELINTOR_AI_GATEWAY_ENDPOINT");

    tauri_build::try_build(tauri_build::Attributes::new()).expect("tauri build setup");
}
