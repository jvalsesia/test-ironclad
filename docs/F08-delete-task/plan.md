# Implementation Plan: Delete Task

**Prerequisites:**
- F04 implemented and merged: the repository's delete operation and its single-statement PostgreSQL implementation, the `src/http/tasks/` route module
- F01 implemented: uniform error types, router fallbacks, the body-limit middleware, the `/__test/slow` diagnostic route, the `#[sqlx::test]` harness
- `docs/F05-get-task-by-id/spec.md` available as the reference for the shared id extractor and not-found helper (whether or not F05 has been merged)
- Rust stable ≥ 1.100 with `rustfmt` and `clippy`
- PostgreSQL 16 reachable with a role allowed to create databases
- No new environment variables, crates, configuration files or compile-checked statements

### Stage 1: Application

**1. Shared Id and Not-Found Helpers** - If F05 has not been merged, add the task id display, the not-found helper and the path extractor exactly as the F05 spec describes. If it has, reuse them unchanged.

**2. Delete Task Use Case** - Implement the use case that removes a task through the existing repository port and reports a task that did not exist through the shared not-found helper, passing repository errors through unchanged.

### Stage 2: HTTP and Documentation

**3. Delete Handler and Route** - Implement the delete endpoint combining the id extractor and the use case, return 204 with an empty body, and merge its route into the task route module next to the other task routes.

**4. Developer Documentation** - Document the delete endpoint and its success and error responses in the README, and update the project status in the project guidance, per spec Section 4.
