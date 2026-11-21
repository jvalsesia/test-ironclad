# Implementation Plan: Structured Logging and Request IDs

**Prerequisites:**
- F01 implemented (router builder, configuration reader, diagnostic routes, test harness)
- Rust stable ≥ 1.100 with `rustfmt` and `clippy`; `sqlx-cli` 0.9.x
- PostgreSQL 16 reachable with a role allowed to create databases (needed by `#[sqlx::test]`)
- New environment variables: `LOG_FORMAT`, `LOG_LEVEL` (defaults and valid values in spec Section 3.1)
- Configuration files: `.env.example` (document both variables); `.env.test` unchanged

### Stage 1: Logging Configuration and Subscriber

**1. Dependencies and Environment Documentation** - Add the new logging and ID-generation dependencies named in spec Section 3.1 and document the two new variables in `.env.example`.

**2. Logging Configuration** - Extend the single configuration reader with the log format and level settings, validating both and naming the offending variable on error, per spec Sections 3.1 and 4.

**3. Subscriber Initializer and Startup Wiring** - Replace the minimal startup subscriber with a configurable one in the infrastructure layer, and reorder the composition root so a configuration error is still reported before the configured subscriber exists, following the startup flow in spec Section 2.

### Stage 2: Request Context Middleware

**4. Request ID Rules** - Implement the request-ID resolution (reuse a valid incoming value, otherwise generate one) and expose the resulting ID to handlers as a request extension, as specified in spec Section 5.1.

**5. Request Span and Completion Line** - Run each request inside a span carrying the ID, set the response header, and emit the single completion event with its level mapping and the health-probe rule, as specified in spec Section 5.2.

**6. Router Integration** - Add the middleware as the outermost layer of the router so every response, including panic and early-rejection responses, is covered, and update the documented layer order.

### Stage 3: Harness and Documentation

**7. Test Harness Support** - Extend the shared integration-test helpers with what log and header assertions need: a raw HTTP helper, UUID-version assertions, and extraction and waiting for completion lines in captured output (spec Section 4).

**8. Documentation Updates** - Update the README with the new variables, the logging formats and the request ID header, and update `CLAUDE.md` with the revised stack list and the logging convention.
