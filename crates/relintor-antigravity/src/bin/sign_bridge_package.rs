use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use relintor_antigravity::{
    plugin_signing_bytes, read_plugin_package_manifest, runtime_target_triple, trusted_key_id,
    verify_plugin_package, PluginPackageFile, PluginPackageManifest, PLUGIN_PACKAGE_MANIFEST_FILE,
};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn usage() -> ! {
    eprintln!(
        "usage: sign_bridge_package --package-dir <dir> --key-file <file> [--version <version>]"
    );
    std::process::exit(2);
}

fn arg(args: &[String], name: &str) -> String {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
        .unwrap_or_else(|| usage())
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn collect(root: &Path, current: &Path, files: &mut Vec<PluginPackageFile>) {
    for entry in fs::read_dir(current).unwrap_or_else(|error| panic!("read package: {error}")) {
        let entry = entry.unwrap_or_else(|error| panic!("read package entry: {error}"));
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .unwrap_or_else(|error| panic!("inspect package entry: {error}"));
        assert!(
            !metadata.file_type().is_symlink(),
            "package contains a link"
        );
        if metadata.is_dir() {
            collect(root, &path, files);
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if relative != PLUGIN_PACKAGE_MANIFEST_FILE {
                files.push(PluginPackageFile {
                    relative_path: relative,
                    sha256: sha256(&fs::read(&path).unwrap()),
                });
            }
        } else {
            panic!("package contains a non-regular entry");
        }
    }
}

fn main() {
    let args = env::args().collect::<Vec<_>>();
    let key_file = PathBuf::from(arg(&args, "--key-file"));
    let key_text = fs::read_to_string(&key_file).expect("read signing key");
    let key_bytes: [u8; 32] = BASE64
        .decode(key_text.trim())
        .expect("decode signing key")
        .try_into()
        .expect("signing key must be 32 bytes");
    let signing_key = SigningKey::from_bytes(&key_bytes);
    if args.get(1).is_some_and(|arg| arg == "public-key") {
        println!(
            "public_key_base64={}",
            BASE64.encode(signing_key.verifying_key().as_bytes())
        );
        println!("key_id={}", trusted_key_id(&signing_key.verifying_key()));
        return;
    }
    let package_dir = PathBuf::from(arg(&args, "--package-dir"));
    if args.get(1).is_some_and(|arg| arg == "verify") {
        let manifest = read_plugin_package_manifest(&package_dir).expect("read package manifest");
        verify_plugin_package(&package_dir, &manifest, &signing_key.verifying_key(), None)
            .expect("verify signed package");
        println!("verified_package={}", manifest.package_id);
        println!("verified_files={}", manifest.files.len());
        return;
    }
    let version = args
        .windows(2)
        .find(|pair| pair[0] == "--version")
        .map(|pair| pair[1].clone())
        .unwrap_or_else(|| "0.1.0".into());
    let mut files = Vec::new();
    collect(&package_dir, &package_dir, &mut files);
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let mut manifest = PluginPackageManifest {
        schema_version: 1,
        package_id: "com.relintor.antigravity".into(),
        package_version: version,
        protocol_version: "relintor-antigravity-hooks-v1".into(),
        target_triple: runtime_target_triple().into(),
        supported_antigravity_versions: vec!["1.".into()],
        files,
        key_id: trusted_key_id(&signing_key.verifying_key()),
        signature: String::new(),
    };
    manifest.signature = BASE64.encode(
        signing_key
            .sign(&plugin_signing_bytes(&manifest).expect("encode signing payload"))
            .to_bytes(),
    );
    fs::write(
        package_dir.join(PLUGIN_PACKAGE_MANIFEST_FILE),
        serde_json::to_vec_pretty(&manifest).expect("encode package manifest"),
    )
    .expect("write package manifest");
    println!("package_id={}", manifest.package_id);
    println!("package_version={}", manifest.package_version);
    println!("target_triple={}", manifest.target_triple);
    println!("key_id={}", manifest.key_id);
    println!("files={}", manifest.files.len());
}
