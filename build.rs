use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=scripts/docs/render_diagrams.sh");
    println!("cargo:rerun-if-changed=docs");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let script = format!("{}/scripts/docs/render_diagrams.sh", manifest_dir);
    let docs_dir = format!("{}/docs", manifest_dir);

    if let Ok(entries) = fs::read_dir(&docs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("mmd") {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }

    if !Path::new(&script).exists() {
        println!("cargo:warning=Docs render script missing at {}", script);
        return;
    }

    let status = Command::new(&script)
        .current_dir(&manifest_dir)
        .env("STRICT_MODE", "0")
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            println!(
                "cargo:warning=Diagram render script exited with status {} (continuing build)",
                s
            );
        }
        Err(err) => {
            println!(
                "cargo:warning=Failed to run diagram render script: {} (continuing build)",
                err
            );
        }
    }
}
