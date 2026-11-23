# Implementation Plan: Get Task by ID

**Prerequisites:**
- F04 implemented and merged: task entity, `TaskRepository::find_by_id`, `PgTaskRepository`, `TaskResponse`, the `src/http/tasks/` route module
- F01 implemented: uniform error type with the `invalid_id` and `not_found` codes, router fallbacks, the `/__test/slow` diagnostic route, the `#[sqlx::test]` harness
- Rust stable ≥ 1.100 with `rustfmt` and `clippy`; `sqlx-cli` 0.9.x
- PostgreSQL 16 reachable with a role allowed to create databases, and migrations applied to the development database named by `DATABASE_URL` so compile-time-checked queries build
- No new environment variables, crates or configuration files

### Stage 1: Application

**1. Task Id Display** - Give the task identifier a canonical text form (lowercase hyphenated) so messages and logs render it the same way everywhere, per spec Section 3.1.

**2. Shared Not-Found Helper** - Add the application-level helper that turns a task identifier into the not-found error carrying the message from spec Section 5, so the update and delete features can reuse it.

**3. Get Task Use Case** - Implement the use case that loads a task through the existing repository port and reports a missing task through the shared helper, passing repository errors through unchanged.

### Stage 2: HTTP

**4. Task Id Path Extractor** - Implement the shared path extractor that accepts only the hyphenated UUID form and rejects every other input with the uniform invalid-id error before any database access, per spec Sections 3 and 3.1.

**5. Get Handler and Route** - Implement the get endpoint that combines the extractor, the use case and the shared task response, and merge its route into the task route module next to the create route.

**6. Developer Documentation** - Document the new endpoint and its success and error responses in the README. Update the project guidance with the new status and the shared id-parsing and not-found helpers, per spec Section 4.

**7. Offline Query Metadata** - Regenerate and commit the offline query metadata for the new compile-checked statements (spec Section 7) so the project still builds without a database.
