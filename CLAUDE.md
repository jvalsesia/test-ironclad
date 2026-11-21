# Task API

Single-tenant REST API for managing tasks. Rust + axum + sqlx + PostgreSQL. No auth.

- Requirements: `docs/prd.md` (source of truth for scope, limits, error codes, acceptance criteria)
- Implementation status: `docs/prd_progress.json` (managed by ironclad skills — do not hand-edit)
- Per-feature docs: `docs/F<ID>-<kebab-name>/{spec.md,plan.md,contract.md}`

## Stack

- Rust stable ≥ 1.100, edition 2024; tokio
- axum 0.8.9; tower / tower-http for middleware
- sqlx 0.9.1 (`postgres`, `runtime-tokio`, `tls-rustls`, `uuid`, `chrono`, `migrate`, `macros`)
- PostgreSQL 16
- serde, thiserror; `tracing` + `tracing-subscriber` for logs; utoipa 5.x for OpenAPI

Do not add new crates for things the stack above already covers. Any new dependency must be justified in the feature's spec.

## Architecture

Clean architecture. **Use the `ironclad:clean-arch` skill for any backend change** (new endpoint, entity, use case, repository, DTO, error, migration, middleware).

- `domain` — entities, value objects, validation, domain errors. Must not import `axum` or `sqlx`.
- `application` — use cases and repository traits.
- `infrastructure` — sqlx repositories, row mapping, DB pool, migrations.
- `http` — routes, handlers, DTOs, error → response mapping, middleware.
- `main.rs` — composition root only (config, pool, migrations, router, graceful shutdown).

Dependencies point inward: `http` → `application` → `domain`; `infrastructure` implements `application` traits.

## Conventions

- **SQL:** use compile-time-checked macros (`sqlx::query!` / `query_as!`). After changing any query or migration, run `cargo sqlx prepare` and commit `.sqlx/`. Builds must work with `SQLX_OFFLINE=true`.
- **Migrations:** `migrations/`, embedded with `sqlx::migrate!` and applied at startup. Never edit an applied migration — add a new one.
- **Errors:** every non-2xx response uses the uniform body `{"error":{"code","message","details":[{"field","message"}]}}` with the codes defined in PRD §6 F01. Never leak SQL or internal details to clients; log them with the request ID.
- **Validation:** lives in the domain layer and reports all invalid fields at once (422 `validation_error`). DB `CHECK` constraints mirror domain rules.
- **Config:** environment variables only (`.env` loaded if present); documented in `.env.example`.
- **Timestamps:** RFC 3339 UTC. Dates: `YYYY-MM-DD`. IDs: UUID.

## Testing

- Unit tests for domain validation next to the code (`#[cfg(test)]`).
- HTTP integration tests in `tests/`, using `#[sqlx::test]` (isolated, migrated DB per test) and driving the real router in-process via `tower::ServiceExt::oneshot`.
- Fixtures: `tests/fixtures/`. Shared test helpers: `tests/common/`.
- Test configuration: `.env.test` (provides `DATABASE_URL` for a local Postgres).
- Local Postgres for tests: `docker compose up db` (once F09 exists) or any Postgres 16 reachable via `DATABASE_URL`.

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
`/ironclad:spec-writer` → `/ironclad:implement-and-evaluate` (or `-tmux` for a whole wave).
Foundation features (F01, F02) must be implemented sequentially before the rest.
