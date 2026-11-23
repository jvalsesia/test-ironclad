# Implementation Plan: List Tasks

**Prerequisites:**
- F04 implemented and merged: the task repository's list operation with its query, filter, sort and page types, the domain status and priority parsers, the shared task response, the `src/http/tasks/` route module
- F01 implemented: uniform error type with the `invalid_query` code, router fallbacks, the `/__test/slow` diagnostic route, the `#[sqlx::test]` harness
- Rust stable ≥ 1.100 with `rustfmt` and `clippy`; `sqlx-cli` 0.9.x
- PostgreSQL 16 reachable with a role allowed to create databases, and migrations applied to the development database named by `DATABASE_URL` so compile-time-checked queries build
- No new environment variables, crates or configuration files

### Stage 1: Application

**1. List Tasks Use Case** - Add the use case that hands a validated list query to the existing repository port and returns the resulting page, passing repository errors through unchanged.

### Stage 2: HTTP

**2. Query Parameter Extractor** - Implement the extractor that turns the raw query string into the list query. It applies the defaults, bounds, strictness rules, messages and fixed detail order from spec Section 3.1, and rejects every invalid parameter at once before any database access.

**3. Page Envelope** - Add the page response that wraps the shared task representation with the total and the effective limit and offset, in the field order from spec Section 5.

**4. List Handler and Route** - Implement the list endpoint combining the extractor, the use case and the envelope, and merge its route into the task route module next to the create route.

### Stage 3: Documentation and Metadata

**5. Developer Documentation** - Document the list endpoint's parameters, defaults, envelope and error example in the README, and update the project guidance with the new status and the location of list query parsing, per spec Section 4.

**6. Offline Query Metadata** - Regenerate and commit the offline query metadata for the new compile-checked seeding statements (spec Section 7) so the project still builds without a database.
