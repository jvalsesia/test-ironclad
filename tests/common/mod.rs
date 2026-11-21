//! Shared integration-test helpers.
//!
//! * In-process tests build the real router against the per-test database
//!   that `#[sqlx::test]` provisions ([`test_router`], [`send`]).
//! * Process-level tests spawn the compiled `task-api` binary with a
//!   controlled environment ([`ServiceProcess`]).

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use sqlx::PgPool;
use sqlx::postgres::PgConnectOptions;
use task_api::http::{AppState, RouterOptions, build_router};
use task_api::infrastructure::db::{PgDiagnosticsProbe, PgTaskRepository};
use tower::ServiceExt;

/// Builds the real router over `pool`, with the diagnostic routes mounted
/// when `test_routes` is true.
pub fn test_router(pool: PgPool, test_routes: bool) -> Router {
    let state = AppState::new(
        Arc::new(PgDiagnosticsProbe::new(pool.clone())),
        Arc::new(PgTaskRepository::new(pool)),
    );
    build_router(state, RouterOptions { test_routes })
}

/// A fully buffered response.
#[derive(Debug)]
pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: String,
}

impl Reply {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or_else(|err| {
            panic!("response body is not JSON ({err}): {}", self.body);
        })
    }
}

/// Sends one request through the router in-process.
pub async fn send(router: &Router, request: Request<Body>) -> Reply {
    let response = router.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    Reply {
        status: parts.status,
        headers: parts.headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

pub fn get(path: &str) -> Request<Body> {
    Request::builder().uri(path).body(Body::empty()).unwrap()
}

pub fn request_with(
    method: &str,
    path: &str,
    content_type: Option<&str>,
    body: &str,
) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(content_type) = content_type {
        builder = builder.header("content-type", content_type);
    }
    builder.body(Body::from(body.to_owned())).unwrap()
}

pub fn post_json(path: &str, body: &str) -> Request<Body> {
    request_with("POST", path, Some("application/json"), body)
}

/// The exact serialized uniform error body.
pub fn uniform_error_body(code: &str, message: &str) -> String {
    format!(r#"{{"error":{{"code":"{code}","message":"{message}","details":[]}}}}"#)
}

/// Asserts status, JSON content type and the exact uniform error body.
pub fn assert_uniform_error(reply: &Reply, status: u16, code: &str, message: &str) {
    assert_eq!(reply.status.as_u16(), status, "body: {}", reply.body);
    assert_eq!(
        reply
            .headers
            .get("content-type")
            .map(|v| v.to_str().unwrap()),
        Some("application/json"),
        "body: {}",
        reply.body
    );
    assert_eq!(reply.body, uniform_error_body(code, message));
}

/// A valid `{"value":"a…a"}` document of exactly `len` bytes.
pub fn json_body_of_len(len: usize) -> String {
    const OVERHEAD: usize = r#"{"value":""}"#.len();
    assert!(len >= OVERHEAD, "length too small for the envelope");
    format!(r#"{{"value":"{}"}}"#, "a".repeat(len - OVERHEAD))
}

/// Connection URL for the per-test database, derived from the server named
/// by `DATABASE_URL` (the test database itself is never the one in the URL).
pub fn database_url_for(options: &PgConnectOptions) -> String {
    let base = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for tests");
    let base = base.split('?').next().unwrap_or(&base);
    let (server, _) = base.rsplit_once('/').expect("DATABASE_URL has no database");
    let database = options.get_database().expect("test database name");
    format!("{server}/{database}")
}

/// A TCP port that was free a moment ago.
pub fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Whether `127.0.0.1:port` currently accepts TCP connections.
pub fn accepts_connections(port: u16) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_ok()
}

static WORKDIR_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// The compiled service running as a child process with a clean
/// environment and a working directory that contains no `.env` file.
pub struct ServiceProcess {
    child: Child,
    lines: Arc<Mutex<Vec<String>>>,
    workdir: PathBuf,
    pub started: Instant,
}

impl ServiceProcess {
    /// Spawns the binary with `env_clear()`, `PATH`, and only `env`.
    pub fn spawn(env: &[(&str, &str)]) -> Self {
        let workdir = std::env::temp_dir().join(format!(
            "task-api-test-{}-{}",
            std::process::id(),
            WORKDIR_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&workdir).unwrap();

        let mut command = Command::new(env!("CARGO_BIN_EXE_task-api"));
        command
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .envs(env.iter().copied())
            .current_dir(&workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();
        let mut child = command.spawn().expect("spawn task-api");

        let lines = Arc::new(Mutex::new(Vec::new()));
        if let Some(stdout) = child.stdout.take() {
            capture(stdout, Arc::clone(&lines));
        }
        if let Some(stderr) = child.stderr.take() {
            capture(stderr, Arc::clone(&lines));
        }
        Self {
            child,
            lines,
            workdir,
            started,
        }
    }

    pub fn pid(&self) -> Pid {
        Pid::from_raw(i32::try_from(self.child.id()).unwrap())
    }

    /// Delivers `signal` to the service process.
    pub fn signal(&self, signal: Signal) {
        kill(self.pid(), signal).expect("deliver signal");
    }

    /// Everything printed so far (stdout and stderr), one entry per line.
    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }

    pub fn output(&self) -> String {
        self.lines().join("\n")
    }

    /// Waits until some line contains `needle`.
    pub fn wait_for_line(&self, needle: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.lines().iter().any(|line| line.contains(needle)) {
                return true;
            }
            thread::sleep(Duration::from_millis(25));
        }
        false
    }

    /// Waits for the process to exit and returns its status.
    pub fn wait_exit(&mut self, timeout: Duration) -> Option<ExitStatus> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().unwrap() {
                // Give the reader threads a moment to drain the pipes.
                thread::sleep(Duration::from_millis(100));
                return Some(status);
            }
            thread::sleep(Duration::from_millis(25));
        }
        None
    }

    /// Whether the process is still running.
    pub fn is_running(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }
}

impl Drop for ServiceProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.workdir);
    }
}

fn capture<R: std::io::Read + Send + 'static>(stream: R, lines: Arc<Mutex<Vec<String>>>) {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            lines.lock().unwrap().push(line);
        }
    });
}

/// Variables every process-level test needs besides `DATABASE_URL`.
pub fn base_env(port: u16) -> Vec<(String, String)> {
    vec![
        ("HOST".to_owned(), "127.0.0.1".to_owned()),
        ("PORT".to_owned(), port.to_string()),
        ("DATABASE_MAX_CONNECTIONS".to_owned(), "10".to_owned()),
        ("DATABASE_ACQUIRE_TIMEOUT_SECS".to_owned(), "5".to_owned()),
    ]
}

/// Spawns the service from owned variable pairs.
pub fn spawn_with(env: &[(String, String)]) -> ServiceProcess {
    let borrowed: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    ServiceProcess::spawn(&borrowed)
}
