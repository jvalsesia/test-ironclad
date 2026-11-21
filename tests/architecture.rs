//! Static checks: layer isolation and compile-time-checked SQL only.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Source lines that mention the forbidden crates (comments are ignored).
fn offending_lines(layer: &str, crates: &[&str]) -> Vec<String> {
    let mut offenders = Vec::new();
    let files = rust_files(&root().join(layer));
    assert!(!files.is_empty(), "no .rs files found under {layer}");
    for file in files {
        for (number, line) in fs::read_to_string(&file).unwrap().lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            if crates.iter().any(|name| {
                code.contains(&format!("use {name}"))
                    || code.contains(&format!("{name}::"))
                    || code.contains(&format!("extern crate {name}"))
            }) {
                offenders.push(format!(
                    "{}:{}: {}",
                    file.display(),
                    number + 1,
                    line.trim()
                ));
            }
        }
    }
    offenders
}

#[test]
fn domain_does_not_reference_axum_or_sqlx() {
    let offenders = offending_lines("src/domain", &["axum", "sqlx"]);
    assert!(offenders.is_empty(), "{offenders:#?}");
}

#[test]
fn application_does_not_reference_axum_or_sqlx() {
    let offenders = offending_lines("src/application", &["axum", "sqlx"]);
    assert!(offenders.is_empty(), "{offenders:#?}");
}

#[test]
fn layer_directories_and_composition_root_exist() {
    for dir in [
        "src/domain",
        "src/application",
        "src/infrastructure",
        "src/http",
    ] {
        assert!(root().join(dir).is_dir(), "{dir} is missing");
    }
    assert!(root().join("src/main.rs").is_file());
}

#[test]
fn sql_uses_compile_time_macros_only() {
    // Built from pieces so this file does not match its own scan.
    let runtime_calls: Vec<String> = ["query", "query_as", "query_scalar"]
        .iter()
        .flat_map(|name| [format!("sqlx::{name}("), format!("sqlx::{name}::<")])
        .collect();

    let mut offenders = Vec::new();
    for dir in ["src", "tests"] {
        for file in rust_files(&root().join(dir)) {
            for (number, line) in fs::read_to_string(&file).unwrap().lines().enumerate() {
                if runtime_calls
                    .iter()
                    .any(|call| line.contains(call.as_str()))
                {
                    offenders.push(format!(
                        "{}:{}: {}",
                        file.display(),
                        number + 1,
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(offenders.is_empty(), "{offenders:#?}");
}

#[test]
fn manifest_pins_the_mandated_stack() {
    let manifest = fs::read_to_string(root().join("Cargo.toml")).unwrap();
    assert!(manifest.contains("edition = \"2024\""));
    assert!(manifest.contains("rust-version = \"1.100\""));
    assert!(manifest.contains("axum = \"0.8.9\""));
    assert!(manifest.contains("version = \"0.9.1\""));
    for feature in [
        "postgres",
        "runtime-tokio",
        "tls-rustls",
        "uuid",
        "chrono",
        "migrate",
        "macros",
    ] {
        assert!(
            manifest.contains(&format!("\"{feature}\"")),
            "sqlx feature {feature}"
        );
    }
}
