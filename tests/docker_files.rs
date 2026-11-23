//! Static shape checks for the container files. They need neither Docker nor
//! a database; behavior that needs the daemon is verified by running the
//! commands in `docs/F09-docker-compose-local-environment/contract.md`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

fn read(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn lines(text: &str) -> Vec<&str> {
    text.lines().map(str::trim).collect()
}

fn from_lines(dockerfile: &str) -> Vec<String> {
    lines(dockerfile)
        .into_iter()
        .filter(|l| l.to_ascii_uppercase().starts_with("FROM "))
        .map(String::from)
        .collect()
}

fn has_line(text: &str, wanted: &str) -> bool {
    lines(text).contains(&wanted)
}

fn assigned(env_text: &str, key: &str) -> Option<String> {
    lines(env_text)
        .iter()
        .find_map(|l| l.strip_prefix(&format!("{key}=")).map(String::from))
}

#[test]
fn dockerfile_is_multi_stage_with_required_bases() {
    let froms = from_lines(&read("Dockerfile"));
    assert!(
        froms.len() >= 2,
        "expected at least two FROM lines: {froms:?}"
    );
    let builder = froms[0].split_whitespace().nth(1).unwrap();
    let tag = builder
        .strip_prefix("rust:")
        .unwrap_or_else(|| panic!("builder base must be rust:, got {builder}"));
    let mut parts = tag.split('.').map(|p| p.parse::<u32>().unwrap());
    let (major, minor) = (parts.next().unwrap(), parts.next().unwrap_or(0));
    assert!((major, minor) >= (1, 100), "rust version too old: {tag}");
    let last = froms.last().unwrap().split_whitespace().nth(1).unwrap();
    assert_eq!(last, "debian:bookworm-slim");
}

#[test]
fn dockerfile_builds_offline() {
    let text = read("Dockerfile");
    assert!(text.contains("SQLX_OFFLINE=true"));
    assert!(text.contains("COPY .sqlx"));
    assert!(text.contains("COPY migrations"));
    assert!(!text.contains("DATABASE_URL"));
}

#[test]
fn dockerfile_runs_as_non_root() {
    let text = read("Dockerfile");
    let after_final_from = text
        .rsplit_once("\nFROM ")
        .map(|(_, rest)| rest)
        .expect("final stage");
    let user = lines(after_final_from)
        .into_iter()
        .find_map(|l| l.strip_prefix("USER ").map(|u| u.trim().to_string()))
        .expect("USER instruction in the final stage");
    assert!(
        user != "root" && user != "0",
        "USER must be non-root: {user}"
    );
}

#[test]
fn dockerfile_uses_exec_form_entrypoint() {
    let text = read("Dockerfile");
    let entry = lines(&text)
        .into_iter()
        .find_map(|l| l.strip_prefix("ENTRYPOINT ").map(String::from))
        .expect("ENTRYPOINT");
    assert!(
        entry.starts_with('['),
        "ENTRYPOINT must be JSON form: {entry}"
    );
    let first = entry
        .trim_start_matches('[')
        .trim_start()
        .trim_start_matches('"');
    let first = first.split('"').next().unwrap();
    assert!(
        first == "task-api" || first.ends_with("/task-api"),
        "{entry}"
    );
}

#[test]
fn compose_defines_db_service() {
    let text = read("docker-compose.yml");
    for needle in [
        "image: postgres:16-alpine",
        "POSTGRES_DB: tasks",
        "POSTGRES_USER: tasks",
        "POSTGRES_PASSWORD: tasks",
        "- \"5432:5432\"",
        "- pgdata:/var/lib/postgresql/data",
        "pg_isready",
        "interval: 5s",
        "timeout: 3s",
        "retries: 10",
    ] {
        assert!(text.contains(needle), "missing in compose: {needle}");
    }
}

#[test]
fn compose_defines_api_service() {
    let text = read("docker-compose.yml");
    assert!(text.contains("dockerfile: Dockerfile"));
    assert!(text.contains("context: ."));
    assert!(text.contains("DATABASE_URL: postgres://tasks:tasks@db:5432/tasks"));
    assert!(text.contains("- \"8080:8080\""));
    assert!(text.contains("restart: unless-stopped"));
    assert!(text.contains("condition: service_healthy"));
}

#[test]
fn compose_declares_named_volume() {
    let text = read("docker-compose.yml");
    let top = text
        .split("\nvolumes:\n")
        .nth(1)
        .expect("top-level volumes section");
    assert!(top.lines().any(|l| l.trim() == "pgdata:"));
}

#[test]
fn dockerignore_excludes_target_and_git() {
    let text = read(".dockerignore");
    assert!(has_line(&text, "target/"));
    assert!(has_line(&text, ".git/"));
}

#[test]
fn dockerignore_keeps_build_inputs() {
    let text = read(".dockerignore");
    for l in lines(&text) {
        for keep in [".sqlx", "migrations", "Cargo.lock"] {
            assert!(!l.contains(keep), "build input excluded: {l}");
        }
    }
}

#[test]
fn env_example_lists_all_variables() {
    let text = read(".env.example");
    for key in [
        "DATABASE_URL",
        "HOST",
        "PORT",
        "DATABASE_MAX_CONNECTIONS",
        "DATABASE_ACQUIRE_TIMEOUT_SECS",
        "LOG_LEVEL",
        "LOG_FORMAT",
    ] {
        let value = assigned(&text, key).unwrap_or_else(|| panic!("{key} not assigned"));
        assert!(!value.is_empty(), "{key} is empty");
    }
}

#[test]
fn env_example_documents_defaults() {
    let text = read(".env.example");
    assert_eq!(assigned(&text, "LOG_FORMAT").as_deref(), Some("json"));
    assert_eq!(assigned(&text, "LOG_LEVEL").as_deref(), Some("info"));
    assert_eq!(assigned(&text, "PORT").as_deref(), Some("8080"));
}
