# Task API

Single-tenant REST API for managing tasks. Rust + axum + sqlx + PostgreSQL. No auth.

- Requirements: `docs/prd.md` (source of truth for scope, limits, error codes, acceptance criteria)
- Implementation status: `docs/prd_progress.json` (managed by ironclad skills — do not hand-edit)
- Per-feature docs: `docs/F<ID>-<kebab-name>/{spec.md,plan.md,contract.md}` (+ `eval-report-<ts>.md` from the evaluator)
- Developer how-to (run, test, env vars): `README.md`

**Status:** F01 (project foundation) is implemented and evaluated `done`. F02 (structured logging and request IDs) is implemented. F03–F10 are not started; build them wave by wave (see Workflow).

## Stack

- Rust stable ≥ 1.100, edition 2024; tokio
- axum 0.8.9; tower-http 0.6 (`catch-panic`)
- sqlx 0.9.1 (`postgres`, `runtime-tokio`, `tls-rustls`, `uuid`, `chrono`, `migrate`, `macros`; default features off)
- PostgreSQL 16
- serde, serde_json, serde_path_to_error, thiserror 2, async-trait, http-body-util, dotenvy
- `tracing` + `tracing-subscriber` (`fmt`, `ansi`, `std`, `json`, `env-filter`) for logs; `uuid` (`v4`) for request IDs; utoipa 5.x for OpenAPI (F10)
- Dev only: reqwest (no TLS), nix (`signal`), futures-util, tower (`util`)

Do not add new crates for things the stack above already covers. Any new dependency must be justified in the feature's spec.

## Architecture

Single Cargo package `task-api` (library `task_api` + binary `task-api`), clean architecture. **Use the `ironclad:clean-arch` skill for any backend change**, but keep this package layout (not the skill's `crates/*` workspace layout).

- `src/domain` — entities, value objects, validation, domain errors (`FieldError`, `DomainError`). Must not reference `axum` or `sqlx` (enforced by `tests/architecture.rs`).
- `src/application` — use cases, ports (`DiagnosticsProbe`; repository traits arrive with F04), `AppError` (`Validation`, `NotFound`, `Unavailable`, `Internal`).
- `src/infrastructure/db` — pool with startup retries, embedded migrator, `map_sqlx_error`, sqlx adapters.
- `src/http` — `router.rs` (`build_router`), `state.rs` (`AppState`), `error.rs` (uniform error body; the only place HTTP statuses for errors are chosen), `extract.rs` (`ApiJson`), `middleware/body_limit.rs`, `middleware/request_context.rs` (outermost layer: request ID, `request` span, completion log line), `test_routes.rs`, `server.rs` (graceful shutdown).
- `src/config.rs` — the only place the environment is read.
- `src/main.rs` — composition root only.

Dependencies point inward: `http` → `application` → `domain`; `infrastructure` implements `application` ports. New features add use cases and ports under `application`, adapters under `infrastructure`, handlers under `http`, and extend `AppState` and `build_router`.

## Conventions

- **SQL:** compile-time-checked macros only (`sqlx::query!` / `query_as!` / `query_scalar!`); `tests/architecture.rs` fails on runtime `sqlx::query(` calls. After changing any query (including in `tests/`) or migration, run `cargo sqlx prepare -- --all-targets` and commit `.sqlx/`. Builds must work with `SQLX_OFFLINE=true`.
- **Compile-time DB:** macros need a migrated dev database while building: set `DATABASE_URL` (or copy `.env.example` to `.env`) and run `cargo sqlx migrate run` once.
- **Migrations:** `migrations/<timestamp>_<name>.sql`, embedded with `sqlx::migrate!` and applied at startup. Never edit an applied migration — add a new one (`build.rs` re-embeds on change).
- **Errors:** every non-2xx response uses `{"error":{"code","message","details":[{"field","message"}]}}`. Create errors via `ApiError` constructors; convert from `AppError` with `?`. Never leak SQL or internal details; causes are logged server-side. Codes/statuses and exact F01 messages are in `src/http/error.rs` and PRD §6 F01.
- **Request bodies:** use `ApiJson<T>` (415 wrong content type, 413 over 65,536 bytes, 400 `invalid_json` naming the field). "64 KB" = 65,536 bytes.
- **Validation:** domain layer, all invalid fields reported at once (422 `validation_error`). DB `CHECK` constraints mirror domain rules. Note `title` is `VARCHAR(200)`, so a 201-char title fails at the column (SQLSTATE 22001) before the CHECK.
- **Config:** environment variables only, read in `config.rs`; `.env` loaded if present; documented in `.env.example`. `ENABLE_TEST_ROUTES=true` mounts `/__test/*` diagnostic routes — never enable outside tests.
- **No `unwrap`/`expect`** outside tests (clippy-denied; `clippy.toml` allows them in tests, and `tests/*.rs` carry a crate-level allow). `unsafe` is forbidden.
- **Logging:** `info!`/`warn!`/`error!` with values formatted into the message. Never log `DATABASE_URL` (it embeds the password), query strings, or request/response bodies. `LOG_FORMAT` (`json`|`pretty`) and `LOG_LEVEL` (`EnvFilter` directive) are read in `config.rs`; the subscriber is installed by `infrastructure/logging.rs` after config parsing. Every request runs in a `request` span (`request_id`) and emits one completion line; `X-Request-Id` is echoed on every response.
- **Timestamps:** RFC 3339 UTC. Dates: `YYYY-MM-DD`. IDs: UUID.

## Testing

- Unit tests next to the code (`#[cfg(test)]`).
- Integration tests in `tests/`: every test touching PostgreSQL is `#[sqlx::test]` (isolated, migrated DB per test; use `(PgPoolOptions, PgConnectOptions)` arguments when you need custom pool settings or a URL for the spawned binary).
  - In-process: `tests/common::test_router(pool, test_routes)` + `send(...)` (real router via `tower::ServiceExt::oneshot`).
  - Process-level: `tests/common::ServiceProcess` spawns the binary with a cleared environment and a `.env`-free working dir; use for startup, exit codes, signals, shutdown.
- Fixtures: `tests/fixtures/` (empty so far). Shared helpers: `tests/common/`.
- Run (needs Postgres 16 with a role that can create databases):
  ```bash
  set -a; . ./.env.test; set +a   # exports DATABASE_URL
  cargo test
  ```
  `.env.test` holds only the local `DATABASE_URL` and is committed. The full suite takes ~50 s (shutdown/startup tests wait on real timeouts). One test needs TCP port 8080 free.
- Local Postgres: `docker run -d --name task-api-pg -p 5432:5432 -e POSTGRES_USER=tasks -e POSTGRES_PASSWORD=tasks -e POSTGRES_DB=tasks postgres:16-alpine` (or `docker compose up db` once F09 exists).

## Quality gates

All must pass before a feature is considered done:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
SQLX_OFFLINE=true cargo build
```

## Workflow

Features are built wave by wave per PRD §8 (Execution Waves) using the ironclad skills:
`/ironclad:spec-writer` → `/ironclad:implement-feature` → `/ironclad:evaluator` (or `/ironclad:implement-and-evaluate`, and `-tmux` for a whole wave).
Foundation features (F01, F02) must be implemented sequentially before the rest. Next up: wave 2 (F04, F02, F03, F09).

Open item: the PRD's F01 AC says an over-long title is rejected "by a CHECK constraint", but `VARCHAR(200)` rejects it first (22001). The F01 contract asserts 22001; decide whether to change the column to `TEXT` or reword the AC.
