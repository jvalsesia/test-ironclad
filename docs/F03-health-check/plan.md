# Implementation Plan: Health Check

**Prerequisites:**
- F01 implemented (the `DiagnosticsProbe` port and Postgres adapter, `AppState`, `build_router`, test harness)
- Rust stable ≥ 1.100 with `rustfmt` and `clippy`; `sqlx-cli` 0.9.x
- PostgreSQL 16 reachable with a role allowed to create databases (needed by `#[sqlx::test]`)
- No new environment variables, dependencies or configuration files

### Stage 1: Application

**1. Health Use Case** - Implement the use case that runs the existing database probe under the 2-second timeout and reduces any error or timeout to a "down" result, logging the cause, as described in spec Sections 3 and 4.

### Stage 2: HTTP

**2. Health Payload** - Implement the response payload with its three fields and field order from spec Section 5, including the crate version.

**3. Health Handler and Route** - Implement `GET /health` returning 200 or 503 with the no-store cache header, building the use case from the shared state, and expose its route function per spec Section 4.

**4. Router Integration** - Merge the health routes into the router unconditionally and declare the new modules in the facades.

### Stage 3: Documentation

**5. Documentation Updates** - Document the endpoint and its two payloads in the README and update the project guidance file with the status and the probe-reuse note.
