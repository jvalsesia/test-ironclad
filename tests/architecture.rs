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

/// Non-comment lines of the layer that mention `utoipa` at all.
fn lines_mentioning(layer: &str, word: &str) -> Vec<String> {
    let mut offenders = Vec::new();
    for file in rust_files(&root().join(layer)) {
        for (number, line) in fs::read_to_string(&file).unwrap().lines().enumerate() {
            let code = line.trim_start();
            if !code.starts_with("//") && code.contains(word) {
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
fn domain_and_application_do_not_reference_utoipa() {
    for layer in ["src/domain", "src/application"] {
        let offenders = lines_mentioning(layer, "utoipa");
        assert!(offenders.is_empty(), "{offenders:#?}");
    }
}

/// Production routes must be registered through `utoipa_axum::routes!` (an
/// `OpenApiRouter`), so every served route is documented in
/// `/openapi.json`. Plain registrations are allowed only in the diagnostic
/// test routes and in the documentation routes themselves. Comment lines
/// are skipped and each file is read up to its first `#[cfg(test)]` (unit
/// tests build plain routers).
#[test]
fn production_routes_are_registered_through_routes_macro() {
    let forbidden = [".route(", ".route_service(", ".nest(", ".nest_service("];
    let http = root().join("src/http");
    let mut offenders = Vec::new();
    for file in rust_files(&http) {
        let relative = file.strip_prefix(&http).unwrap();
        if relative == Path::new("test_routes.rs") || relative.starts_with("openapi") {
            continue;
        }
        for (number, line) in fs::read_to_string(&file).unwrap().lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("#[cfg(test)]") {
                break;
            }
            if code.starts_with("//") {
                continue;
            }
            if forbidden.iter().any(|call| code.contains(call)) {
                offenders.push(format!(
                    "{}:{}: {}",
                    file.display(),
                    number + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "routes registered outside utoipa_axum::routes! are missing from /openapi.json; \
         annotate the handler with #[utoipa::path] and register it with routes!: {offenders:#?}"
    );
}
