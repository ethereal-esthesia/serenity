use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=scripts/docs/render_diagrams.sh");
    println!("cargo:rerun-if-changed=docs/runtime-input-control-flow.mmd");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let script = format!("{}/scripts/docs/render_diagrams.sh", manifest_dir);

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
