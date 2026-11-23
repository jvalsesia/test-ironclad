# Implementation Plan: OpenAPI Documentation and Swagger UI

**Prerequisites:**
- F03–F08 implemented and merged: the health handler, the five task handlers and their route files, the task DTOs, the page envelope, the list query extractor and the uniform error type
- F01/F02 implemented: router composition with fallbacks and layers, request-context middleware, the `/__test/slow` diagnostic route, the `#[sqlx::test]` harness and `tests/common` helpers
- Rust stable ≥ 1.100 with `rustfmt` and `clippy`; PostgreSQL 16 reachable with a role allowed to create databases (`.env.test`)
- New crates (spec Section 3.1, item 8): `utoipa` 5.5, `utoipa-axum` 0.2, `utoipa-swagger-ui` 9.0.2 with vendored assets; dev-only `jsonschema` 0.30
- No new environment variables, migrations or queries

### Stage 1: Schema Foundations

**1. Dependencies** - Add the documentation crates and the dev-only validator to the manifest with the features listed in the spec, and confirm the dependency graph still resolves to a single axum version.

**2. Doc-only Schema Types** - Create the OpenAPI module in the HTTP layer with the published status and priority enums, the inline health and sort/order enums, and a check that the enum values match what the domain accepts.

**3. Response and Error Schemas** - Derive schemas on the task representation, page envelope, health payload and uniform error types, publishing them under the PRD names with the constraints, nullability and inline parts from spec Section 5.3.

**4. Request and Parameter Schemas** - Derive schemas on the create and update requests (including the update request's three-state fields) and add the documented list query parameters, keeping their bounds and defaults aligned with the existing parser constants.

### Stage 2: Documented Operations

**5. Operation Annotations** - Annotate the six handlers with path, tag, operation id, parameters, request body, every documented response status from spec Section 3.1, the `Location` header and the examples.

**6. Route Registration** - Convert each endpoint's `routes()` and the task route module to register handlers through the documented router, so route paths come from the annotations, merging the per-method files on the shared id path as before.

**7. Document Root and Router Composition** - Define the document root (title, tags, the nine named schemas) and change `build_router` to build the documented router, split it into the axum router and the document, then mount the docs routes and the optional test routes outside the documented part, keeping layer order and signature unchanged.

### Stage 3: Serving the Documentation

**8. Spec Endpoint** - Serve the document built at router construction at `GET /openapi.json` as JSON.

**9. Swagger UI Endpoints** - Serve the vendored Swagger UI at `/docs`, `/docs/` and `/docs/{*rest}` with the configuration from spec Section 5.2: the index answers 200 directly with asset URLs resolving under `/docs/`, unknown files map to the uniform 404, and serve failures map to the uniform 500.

### Stage 4: Guards, Fixtures and Documentation

**10. Route and Layer Guards** - Extend the static architecture checks so the domain and application layers never reference the documentation crate, and production route files cannot register routes outside the documented router (spec Section 7).

**11. Validator Fixture and Helpers** - Commit the official OpenAPI 3.1 JSON Schema as a fixture and add shared helpers to fetch the served document, resolve references and validate a JSON value against a named component schema.

**12. Developer Documentation** - Add an API documentation section to the README and update the project guidance with the F10 status, the new OpenAPI module and the rule that new endpoints are registered through annotated handlers.
