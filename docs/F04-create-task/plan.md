# Implementation Plan: Create Task

**Prerequisites:**
- F01 implemented (the `tasks` table and migration, `ApiJson`, `ApiError`, `AppError`, `build_router`, test harness)
- Rust stable ≥ 1.100 with `rustfmt` and `clippy`; `sqlx-cli` 0.9.x
- PostgreSQL 16 reachable with a role allowed to create databases (needed by `#[sqlx::test]`), and the migrations applied to the development database named by `DATABASE_URL` so compile-time-checked queries can build
- No new environment variables
- Configuration files: none new; `.env.test` unchanged

### Stage 1: Domain Model

**1. Dependencies** - Add the direct date/time and UUID dependencies named in spec Section 3.1 so the domain can use typed identifiers and dates.

**2. Task Value Objects** - Implement the title, description, status, priority and due-date value objects with their trimming rules, boundaries and exact error messages from spec Section 3.1, so the same rules can be reused by the update feature.

**3. Task Entity and Creation Aggregate** - Add the persisted task entity, its identifier, and the creation aggregate that applies defaults and reports every invalid field at once in the documented order, per spec Section 4.

### Stage 2: Application and Persistence

**4. Repository Port and Query Types** - Define the task repository port with all five operations and the supporting query, patch and page types described in spec Section 5, following the semantics in spec Section 3.1.

**5. Create Task Use Case** - Implement the use case that validates through the creation aggregate and persists only valid input, returning application errors unchanged.

**6. Row Mapping** - Implement the conversion from a stored row to the task entity, turning unexpected stored values into internal application errors.

**7. PostgreSQL Repository Adapter** - Implement all five repository operations with compile-time-checked queries, including the ordering, filtering and pagination rules and the single-statement create and update described in spec Section 3.1, mapping database errors through the existing classification.

### Stage 3: HTTP

**8. Request and Response DTOs** - Implement the create request and the shared task response with millisecond UTC timestamps and the field order from spec Section 5.

**9. Create Handler and Task Routes** - Implement the create endpoint returning 201 with its location header, and the task route module that later endpoints extend, per spec Section 4.

**10. State and Composition Wiring** - Give the shared state the task repository, merge the task routes into the router, and construct the PostgreSQL repository in the composition root.

### Stage 4: Harness and Documentation

**11. Test Harness Support** - Extend the shared integration-test helpers so the test router uses the real repository and tests can read stored rows and counts directly, per spec Section 4.

**12. Offline Metadata and Documentation** - Regenerate and commit the offline query metadata for all targets, and update the README and CLAUDE.md with the new endpoint, stack and task-feature conventions.
