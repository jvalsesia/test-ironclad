# Task API

A small REST service for managing tasks, written in Rust with
[axum](https://github.com/tokio-rs/axum), [sqlx](https://github.com/launchbadge/sqlx)
and PostgreSQL 16. Requirements live in [`docs/prd.md`](docs/prd.md); per-feature
specs, plans and contracts live in `docs/F<ID>-*/`.

This repository currently contains the project foundation (F01): configuration,
database pool and migrations, the uniform error contract, request limits,
graceful shutdown and the integration-test harness. Business endpoints arrive
with later features.

## Prerequisites

- Rust stable ≥ 1.100 with the `rustfmt` and `clippy` components
- PostgreSQL 16 whose role may create and drop databases (needed by `#[sqlx::test]`)
- `sqlx-cli` 0.9.x for migrations and offline metadata:
  `cargo install sqlx-cli --no-default-features --features rustls,postgres`

A throwaway local database:

```bash
docker run -d --name task-api-pg -p 5432:5432 \
  -e POSTGRES_USER=tasks -e POSTGRES_PASSWORD=tasks -e POSTGRES_DB=tasks \
  postgres:16-alpine
```

## Configuration

All configuration comes from environment variables. A `.env` file in the
working directory is loaded when present; real environment variables win.
`.env.example` documents every variable.

| Variable | Default | Valid values |
|---|---|---|
| `DATABASE_URL` | — (required) | PostgreSQL connection string |
| `HOST` | `0.0.0.0` | IPv4 or IPv6 address |
| `PORT` | `8080` | 1–65535 |
| `DATABASE_MAX_CONNECTIONS` | `10` | 1–100 |
| `DATABASE_ACQUIRE_TIMEOUT_SECS` | `5` | 1–300 |
| `ENABLE_TEST_ROUTES` | `false` | `true` / `false` |

`ENABLE_TEST_ROUTES=true` mounts the `/__test/*` diagnostic routes used by the
test suite (`db-ping`, `sql-error`, `panic`, `slow`, `echo`). Leave it `false`
anywhere that is not an automated test.

An invalid or missing value stops startup with exit code `1` and a message
naming the variable.

## Run the service

```bash
cp .env.example .env          # sets DATABASE_URL for the local database above
cargo run
```

On start the service connects to PostgreSQL (up to 5 attempts, 2 s apart),
applies pending migrations, then listens on `HOST:PORT`. `SIGINT`/`SIGTERM`
stop accepting connections, give in-flight requests up to 10 s to finish, then
exit `0`.

Every error response has the same JSON shape:

```json
{"error":{"code":"not_found","message":"Route not found","details":[]}}
```

## API

### `POST /tasks`

Creates a task. Body (`Content-Type: application/json`, at most 65,536 bytes):

| Field | Rules |
|-------|-------|
| `title` | required, trimmed, 1-200 characters |
| `description` | optional, trimmed, at most 5000 characters; blank becomes `null` |
| `status` | optional, `todo` (default), `in_progress`, `done` |
| `priority` | optional, `low`, `medium` (default), `high` |
| `due_date` | optional, `YYYY-MM-DD`, a real calendar date |

```bash
curl -i -X POST localhost:8080/tasks -H 'Content-Type: application/json' \
  -d '{"title":"Write PRD","priority":"high","due_date":"2026-12-01"}'
```

Response `201` with `Location: /tasks/{id}`:

```json
{"id":"3f2b6f0e-6d4a-4c8e-9a0f-1b2c3d4e5f60","title":"Write PRD","description":null,"status":"todo","priority":"high","due_date":"2026-12-01","created_at":"2026-11-21T18:11:00.123Z","updated_at":"2026-11-21T18:11:00.123Z"}
```

Errors: `400 invalid_json`, `413 payload_too_large`, `415 unsupported_media_type`,
`422 validation_error` (one `details` entry per invalid field), `503 service_unavailable`.

## Run the tests

The tests need a running PostgreSQL and the variables from `.env.test`
(`DATABASE_URL`) in the environment of `cargo test`:

```bash
set -a; . ./.env.test; set +a
cargo test
```

Each database test runs against its own isolated database created by
`#[sqlx::test]`. Process-level tests (`tests/startup.rs`, `tests/shutdown.rs`)
spawn the compiled binary; `default_bind_address_is_0_0_0_0_8080` needs TCP
port 8080 to be free.

Compile-time-checked queries (`sqlx::query!`) need a migrated development
database while you build. Apply the migrations to the database named in
`DATABASE_URL` once:

```bash
cargo sqlx migrate run
```

## Quality gates

All four must pass:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
SQLX_OFFLINE=true cargo build
```

## Offline query metadata

Queries are checked at compile time. The metadata that lets the project build
without a database is committed in `.sqlx/`. After adding or changing any
query (including those in `tests/`) or any migration, regenerate and commit it:

```bash
cargo sqlx prepare -- --all-targets
```

Never edit a migration that has been applied; add a new one with
`cargo sqlx migrate add <name>`.

## Layout

```
src/domain          entities, value objects, validation (no axum / sqlx)
src/application     use cases and ports
src/infrastructure  sqlx adapters, pool, migrations
src/http            router, error mapping, extractors, middleware
src/config.rs       the only place the environment is read
src/main.rs         composition root
migrations/         embedded SQL migrations
tests/              integration tests and shared helpers (tests/common)
```
