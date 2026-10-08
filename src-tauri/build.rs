use std::path::PathBuf;

/// The full gaming glossary is private (repository `hark-glossary`). A build
/// without it uses the small public example, so anyone can still compile Hark.
fn glossary() {
    println!("cargo:rerun-if-env-changed=HARK_GLOSSARY");
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let full = std::env::var_os("HARK_GLOSSARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("../../hark-glossary/glossary.json"));
    let example = manifest.join("../server/glossary.example.json");
    let source = if full.exists() {
        full
    } else {
        println!("cargo:warning=full glossary not found at {}, building with the example", full.display());
        example
    };
    println!("cargo:rerun-if-changed={}", source.display());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("glossary.json");
    std::fs::copy(&source, out).expect("copy glossary");
}

fn main() {
    glossary();
    tauri_build::build()
}
