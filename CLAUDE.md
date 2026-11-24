# Task API

Single-tenant REST API for managing tasks. Rust + axum + sqlx + PostgreSQL. No auth.

- Requirements: `docs/prd.md` (source of truth for scope, limits, error codes, acceptance criteria)
- Implementation status: `docs/prd_progress.json` (managed by ironclad skills — do not hand-edit)
- Per-feature docs: `docs/F<ID>-<kebab-name>/{spec.md,plan.md,contract.md}` (+ `eval-report-<ts>.md` from the evaluator)
- Developer how-to (run, test, env vars): `README.md`

**Status:** F01 (foundation), F02 (structured logging and request IDs), F03 (health check: `GET /health`, reuses `DiagnosticsProbe::ping` under a 2 s timeout in `src/application/health.rs`), F04 (create task: `POST /tasks`, domain task model, `TaskRepository` with all five operations) and F09 (Docker Compose environment) are implemented, evaluated `done` and merged. Wave 3 is implemented: F05 (get task by id: `GET /tasks/{id}`, shared `TaskIdPath` extractor and `task_not_found` helper), F06 (list tasks: `GET /tasks`), F07 (update task: `PATCH /tasks/{id}`) and F08 (delete task: `DELETE /tasks/{id}`, `DeleteTask` use case). Wave 4 is implemented: F10 (OpenAPI documentation: `GET /openapi.json`, Swagger UI at `GET /docs`).

## Stack

- Rust stable ≥ 1.100, edition 2024; tokio
- axum 0.8.9; tower-http 0.6 (`catch-panic`)
- sqlx 0.9.1 (`postgres`, `runtime-tokio`, `tls-rustls`, `uuid`, `chrono`, `migrate`, `macros`; default features off)
- PostgreSQL 16
- chrono (default features off; `std`, `clock`), uuid (`serde`, `v4`)
- serde, serde_json, serde_path_to_error, thiserror 2, async-trait, http-body-util, dotenvy
- `tracing` + `tracing-subscriber` (`fmt`, `ansi`, `std`, `json`, `env-filter`) for logs; `uuid` for request IDs; utoipa 5.x for OpenAPI (F10)
- Dev only: reqwest (no TLS), nix (`signal`), futures-util, tower (`util`)

Do not add new crates for things the stack above already covers. Any new dependency must be justified in the feature's spec.

## Architecture

Single Cargo package `task-api` (library `task_api` + binary `task-api`), clean architecture. **Use the `ironclad:clean-arch` skill for any backend change**, but keep this package layout (not the skill's `crates/*` workspace layout).

- `src/domain` — entities, value objects, validation, domain errors (`FieldError`, `DomainError`). Must not reference `axum` or `sqlx` (enforced by `tests/architecture.rs`).
- `src/application` — use cases, ports (`DiagnosticsProbe`, `TaskRepository` plus `CreateTask` under `application/tasks`), `AppError` (`Validation`, `NotFound`, `Unavailable`, `Internal`).
- `src/infrastructure/db` — pool with startup retries, embedded migrator, `map_sqlx_error`, sqlx adapters.
- `src/http` — `router.rs` (`build_router`), `state.rs` (`AppState`), `error.rs` (uniform error body; the only place HTTP statuses for errors are chosen), `extract.rs` (`ApiJson`), `middleware/body_limit.rs`, `middleware/request_context.rs` (outermost layer: request ID, `request` span, completion log line), `test_routes.rs`, `server.rs` (graceful shutdown), `openapi/` (F10: `ApiDoc` document root and `docs_routes` in `mod.rs`, doc-only enum schemas and examples in `schemas.rs`, Swagger UI handlers in `swagger.rs`).
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

## Task feature (F04)

- Domain `src/domain/task`: value objects in `fields.rs` own the validation messages (shared with F07); `NewTask::parse` reports every invalid field in order `title`, `description`, `status`, `priority`, `due_date`; the entity file is `task_entity.rs` (a `task.rs` inside `task/` trips clippy `module_inception`) and entities are rebuilt with `Task::restore(TaskParts)`.
- `TaskRepository` (create, find_by_id, list, update, delete) is implemented by `PgTaskRepository`; `list` uses eight static `query_as!` statements via a private macro. Id and timestamps are generated by PostgreSQL.
- HTTP: `src/http/tasks/` merges one `routes()` per endpoint; `TaskResponse` is the single task representation (millisecond UTC timestamps). `AppState.tasks` holds the repository; later features add use cases built in handlers, not state fields.
- Id-addressed endpoints (F05, reused by F07/F08): take the id with `http::tasks::TaskIdPath` (36-char hyphenated UUID only, any other input incl. invalid UTF-8 is `400 invalid_id` "Task id must be a valid UUID", checked before DB access) and report a missing task with `application::tasks::task_not_found(id)` (`404` "Task <lowercase id> not found"). `TaskId` implements `Display` (lowercase hyphenated). Each method on `/tasks/{id}` has its own route file, merged on the same path.
- List (F06): `GET /tasks` query parsing lives in the HTTP extractor `src/http/tasks/list_params.rs` (`ListTasksParams` → `TaskQuery`; strict: repeated/empty/case-mismatched values rejected, unknown names ignored, all errors at once as `400 invalid_query` in fixed order `status`, `priority`, `sort`, `order`, `limit`, `offset`). The envelope is `TaskPageResponse` in `page.rs`; the use case is `ListTasks`. `tests/tasks_list.rs` seeds tied and 10,000-row sets with local `INSERT … generate_series` statements.
- Tests: `tests/common` has `count_tasks` and `stored_task` to read rows directly. `uuid` now has `v4` (request IDs, F02), so tests may generate UUIDs.

## Update feature (F07)

- Domain `task_changes.rs`: `TaskChanges::parse(RawTaskChanges)` is the PATCH validator. Each field is a three-state `RawField` (absent / null / value); `null` clears `description`/`due_date` and is `cannot be null` for `title`/`status`/`priority`; values reuse the F04 value objects; errors in field order; all-absent is `DomainError::NoChanges` → `AppError::NoChanges` → 422 `At least one updatable field must be provided` with `details: []`.
- `UpdateTask` maps `TaskChanges` to `TaskPatch`. `PgTaskRepository::update` sets `updated_at = GREATEST(now(), updated_at + 1 ms)`, so it strictly increases at the millisecond precision shown in responses.
- HTTP: `UpdateTaskRequest` accepts only a JSON object (custom map visitor; a derived struct with all-default fields would accept `[]`).

## OpenAPI feature (F10)

- The document is generated by utoipa from code. Every production route is an annotated handler (`#[utoipa::path(method, path = …, tag, operation_id, …)]`) registered with `utoipa_axum::routes!` in its file's `routes() -> OpenApiRouter<AppState>`; `build_router` merges them into `OpenApiRouter::with_openapi(ApiDoc::openapi())`, calls `split_for_parts()`, then merges `openapi::docs_routes(doc)` and the optional test routes outside the documented part (so the document is identical with `ENABLE_TEST_ROUTES` on or off).
- Guard: `tests/architecture.rs::production_routes_are_registered_through_routes_macro` fails on `.route(`/`.route_service(`/`.nest(`/`.nest_service(` in `src/http` outside `test_routes.rs` and `openapi/` (comments skipped, scan stops at `#[cfg(test)]`). `tests/openapi.rs` pins the six operations, statuses per operation, the nine component schemas (`Task`, `CreateTaskRequest`, `UpdateTaskRequest`, `TaskPage`, `TaskStatus`, `TaskPriority`, `ErrorResponse`, `ErrorDetail`, `HealthResponse`), constraints, parameters and examples; it validates the document against `tests/fixtures/openapi-3.1-schema.json` (official OAS 3.1 schema, verbatim) with the dev-only `jsonschema`. `tests/openapi_conformance.rs` checks live responses against the schemas (`tests/common::validate_against`). Adding an endpoint means updating those expectations.
- Rust types keep their names; published names come from `#[schema(as = …)]` (`TaskResponse → Task`, `TaskPageResponse → TaskPage`, `ErrorEnvelope → ErrorResponse`). Enum schemas are doc-only types in `src/http/openapi/schemas.rs` (unit-tested against the domain and query parsers); domain and application never mention `utoipa` (architecture test). Field descriptions come from `///` doc comments (field-level `description = …` is not supported by utoipa); response examples use `content((Type = "application/json", example = fn))` because the bare `example =` on a response accepts only `json!`/literals.
- `/docs` is served by our own handlers over `utoipa_swagger_ui::serve` (vendored Swagger UI 5.17.14, no network at build): `GET /docs` answers 200 directly with `<base href="/docs/">` injected, unknown files are the uniform 404.

## Testing

- Unit tests next to the code (`#[cfg(test)]`).
- Integration tests in `tests/`: every test touching PostgreSQL is `#[sqlx::test]` (isolated, migrated DB per test; use `(PgPoolOptions, PgConnectOptions)` arguments when you need custom pool settings or a URL for the spawned binary).
  - In-process: `tests/common::test_router(pool, test_routes)` + `send(...)` (real router via `tower::ServiceExt::oneshot`).
  - Process-level: `tests/common::ServiceProcess` spawns the binary with a cleared environment and a `.env`-free working dir; use for startup, exit codes, signals, shutdown.
- Fixtures: `tests/fixtures/` (`openapi-3.1-schema.json`, F10). Shared helpers: `tests/common/` (incl. `openapi_spec`, `resolve`, `validate_against`).
- Run (needs Postgres 16 with a role that can create databases):
  ```bash
  set -a; . ./.env.test; set +a   # exports DATABASE_URL
  cargo test
  ```
  `.env.test` holds only the local `DATABASE_URL` and is committed. The full suite takes ~50 s (shutdown/startup tests wait on real timeouts). One test needs TCP port 8080 free.
- Local Postgres: `docker run -d --name task-api-pg -p 5432:5432 -e POSTGRES_USER=tasks -e POSTGRES_PASSWORD=tasks -e POSTGRES_DB=tasks postgres:16-alpine` (or `docker compose up -d db`; then `export DATABASE_URL=postgres://tasks:tasks@localhost:5432/tasks` and `SQLX_OFFLINE=true cargo test`).
- Docker: `docker compose up --build` runs db + api (host ports 5432 and 8080 must be free); `down` keeps volume `pgdata`, `down -v` wipes it. Compose project `task-api`, image `task-api:local`; compose uses explicit `environment:` only (never `.env`). `Dockerfile` builds offline (`SQLX_OFFLINE=true`, committed `.sqlx/`), runtime is `debian:bookworm-slim`, non-root, under 150 MB. `tests/docker_files.rs` statically guards these files.

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
Foundation features (F01, F02) must be implemented sequentially before the rest; both are merged. Waves 3 and 4 (F05–F08, F10) are implemented; every PRD feature now exists.

Parallel features (`/ironclad:implement-and-evaluate-tmux`) all edit the same shared files, so expect merge conflicts after the first PR lands: the `**Status:**` line above, `README.md` sections, `.env.example`, `src/application/mod.rs`, `src/http/router.rs` (route merges), `tests/common/mod.rs`, `Cargo.toml` features and `docs/prd_progress.json`. Resolve by keeping both sides, then run the quality gates on the merged tree and merge PRs one at a time. Tell each team to `/exit` after its PR is opened, or the wave never sees it finish.