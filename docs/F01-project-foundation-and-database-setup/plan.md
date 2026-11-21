# Implementation Plan: Project Foundation and Database Setup

**Prerequisites:**
- Rust stable ≥ 1.100 with `rustfmt` and `clippy` components
- `sqlx-cli` 0.9.x (`cargo install sqlx-cli --no-default-features --features rustls,postgres`) for creating migrations and running `cargo sqlx prepare`
- PostgreSQL 16 reachable from the host, with a role allowed to create databases (needed by `#[sqlx::test]`)
- Environment variables: `DATABASE_URL` (required), `HOST`, `PORT`, `DATABASE_MAX_CONNECTIONS`, `DATABASE_ACQUIRE_TIMEOUT_SECS`, `ENABLE_TEST_ROUTES` (see spec Section 3.1 for defaults and ranges)
- Configuration files: `.env.example` (documented defaults), `.env.test` (test `DATABASE_URL`), optional local `.env`

### Stage 1: Project Scaffolding and Configuration

**1. Cargo Package and Repository Setup** - Create the `task-api` package (library plus binary) with the edition, minimum Rust version, pinned dependencies, and lint policy described in spec Sections 3.1 and 4. Extend the ignore rules and add `.env.example`, `.env.test`, and the empty fixture folder.

**2. Layered Module Skeleton** - Create the `domain`, `application`, `infrastructure`, and `http` module trees with facade-only module files, the shared domain field-error type, the application error type, and the diagnostics port, following the layout in spec Section 4.

**3. Environment Configuration** - Implement the single configuration reader that loads `.env` when present, applies defaults, validates every variable's range, and produces an error that names the offending variable, per spec Sections 3.1 and 4.

**4. Startup Logging Bootstrap** - Install the minimal stdout logging subscriber at the start of the composition root so that startup, migration, listening, and shutdown messages are emitted with the wording listed in spec Section 3.1.

### Stage 2: Database Foundation

**5. Tasks Table Migration** - Add the first migration that creates the `tasks` table with its defaults, named check constraints, and secondary indexes exactly as defined in spec Section 6.

**6. Connection Pool with Startup Retries** - Build the pool from configuration with a bounded number of connection attempts and a fixed pause between them, logging each attempt and the final failure without ever exposing the connection URL.

**7. Embedded Migration Runner** - Embed the migrations directory into the binary and apply pending migrations at startup before the listener binds, reporting how many were applied and surfacing failures with the migration version and cause.

**8. Database Error Mapping and Diagnostics Adapter** - Classify database errors into unavailable versus internal application errors, and implement the diagnostics port against the pool with compile-time-checked queries. Generate and commit the offline query metadata so the build works without a database.

### Stage 3: HTTP Foundation

**9. Uniform Error Response** - Implement the error-code catalog and the single conversion from application errors and extractor rejections into the uniform JSON error body, as specified in spec Sections 5.1 and 5.2, ensuring internal causes are logged but never returned.

**10. JSON Extractor and Body Limits** - Implement the shared JSON extractor (content type, bounded buffering, parse errors) and the pre-routing body-size middleware so oversized, mistyped, or malformed bodies map to the uniform errors in spec Section 5.3.

**11. Router Composition** - Assemble the router builder with shared state, the unknown-route and wrong-method fallbacks, the panic-containment layer, the body-limit layer, and conditional mounting of the diagnostic routes, as laid out in spec Sections 2 and 5.3.

**12. Diagnostic Routes** - Implement the database ping, induced SQL error, induced panic, slow request, and JSON echo routes from spec Section 5.4, reachable only when the test-routes flag is enabled.

### Stage 4: Server Lifecycle and Developer Experience

**13. Composition Root and Graceful Shutdown** - Wire configuration, pool, migrations, diagnostics adapter, state, router, and listener in the entry point; serve until SIGTERM or SIGINT, drain in-flight requests within the deadline, close the pool, and map every startup failure to exit code 1, following the startup flow in spec Section 2.

**14. Integration Harness Support** - Provide the shared harness helpers described in spec Section 7 so later features can build the router against an isolated database, issue requests in-process, assert the uniform error body, and spawn the service binary with a controlled environment.

**15. Developer Documentation and Final Metadata** - Write the README covering configuration, running the service, running the test suite with the test environment file, the quality gates, and refreshing offline query metadata; regenerate the offline metadata for all targets so it matches every committed query.
