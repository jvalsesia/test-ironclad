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

## Docker

Docker Engine with Compose v2+ runs the whole stack. Host ports **5432**
(PostgreSQL) and **8080** (API) must be free.

```bash
docker compose up --build   # build the api image, start db, wait for healthy, start api
docker compose up db        # only PostgreSQL on localhost:5432 (for cargo run / cargo test)
docker compose down         # stop and remove containers; keep the pgdata volume
docker compose down -v      # also delete the pgdata volume (wipes all data)
```

The `api` image (`task-api:local`) is built offline from the committed `.sqlx/`
metadata, runs as a non-root user and is under 150 MB. The stack ignores `.env`;
containers use the explicit `environment:` entries in `docker-compose.yml`.

Run the tests against the compose database (it has no `tasks` table for
compile-time query checking, hence offline mode):

```bash
docker compose up -d db
export DATABASE_URL=postgres://tasks:tasks@localhost:5432/tasks
SQLX_OFFLINE=true cargo test
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
| `LOG_LEVEL` | `info` | `error` / `warn` / `info` / `debug` / `trace` (behavior arrives with F02) |
| `LOG_FORMAT` | `json` | `json` / `text` (behavior arrives with F02) |

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
