use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};
fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    assert_eq!(
        env::var("CARGO_CFG_TARGET_ARCH").as_deref(),
        Ok("x86_64"),
        "The bundled ConPTY runtime currently supports Windows x64 only"
    );
    let vendor = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../vendor/microsoft-conpty");
    let manifest = vendor.join("manifest.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).expect("ConPTY manifest"))
            .expect("valid ConPTY manifest");
    // OUT_DIR is <target>/<profile>/build/<crate>/out, including cross-compilation targets.
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let profile = out.ancestors().nth(3).expect("Cargo profile directory");
    for entry in metadata["files"].as_array().expect("ConPTY files") {
        let source = vendor.join(entry["path"].as_str().unwrap());
        println!("cargo:rerun-if-changed={}", source.display());
        let bytes = fs::read(&source).expect("vendored ConPTY binary");
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            entry["sha256"].as_str().unwrap(),
            "ConPTY binary hash mismatch: {}",
            source.display()
        );
        for directory in [profile.to_path_buf(), profile.join("deps")] {
            fs::create_dir_all(&directory).expect("Cargo output directory");
            let destination = directory.join(source.file_name().unwrap());
            if fs::read(&destination).ok().as_deref() != Some(bytes.as_slice()) {
                fs::write(destination, &bytes).expect("copy ConPTY beside binaries and tests");
            }
        }
    }
}
