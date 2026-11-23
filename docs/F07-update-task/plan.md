# Implementation Plan: Update Task

**Prerequisites:**
- F04 implemented and merged: task value objects and messages, the repository's update operation and patch types, the single-statement update in the PostgreSQL adapter, the shared task response, the `src/http/tasks/` route module
- F01 implemented: uniform error types, the JSON extractor (415/413/400), the body-limit middleware, the `/__test/slow` diagnostic route, the `#[sqlx::test]` harness
- `docs/F05-get-task-by-id/spec.md` available as the reference for the shared id extractor and not-found helper (whether or not F05 has been merged)
- Rust stable ≥ 1.100 with `rustfmt` and `clippy`; `sqlx-cli` 0.9.x
- PostgreSQL 16 reachable with a role allowed to create databases, and migrations applied to the development database named by `DATABASE_URL` so compile-time-checked queries build
- No new environment variables, crates or configuration files

### Stage 1: Domain and Errors

**1. Update Aggregate** - Implement the domain update aggregate that tells absent, null and present fields apart, applies the field rules and messages from spec Section 3.1 using the existing value objects, and reports every invalid field at once in the documented order.

**2. No-Changes Error Path** - Add the no-changes variants to the domain and application errors and map them in the HTTP error module to the 422 response with the PRD message and no details, per spec Section 3.

### Stage 2: Application and Persistence

**3. Shared Id and Not-Found Helpers** - If F05 has not been merged, add the task id display, the not-found helper and the path extractor exactly as the F05 spec describes. If it has, reuse them unchanged.

**4. Update Task Use Case** - Implement the use case that validates through the update aggregate, translates the result into the repository's patch, performs the update, and reports a missing task through the shared helper.

**5. Monotonic Update Timestamp** - Change the existing update statement so the update timestamp always advances by at least one millisecond, per spec Section 6.

### Stage 3: HTTP

**6. Update Request Body** - Add the update request DTO whose fields tell absent from null from a value, keep wrong-type errors naming the field, and convert into the domain's raw update input.

**7. Update Handler and Route** - Implement the update endpoint combining the id extractor, the JSON extractor and the use case, return the shared task response, and merge its route into the task route module.

### Stage 4: Documentation and Metadata

**8. Developer Documentation** - Document the update endpoint's field semantics and error responses in the README, and update the project guidance with the new status, the update validator and the timestamp rule, per spec Section 4.

**9. Offline Query Metadata** - Regenerate and commit the offline query metadata for the changed update statement so the project still builds without a database.
