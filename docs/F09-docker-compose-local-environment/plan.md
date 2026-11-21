# Implementation Plan: Docker Compose Local Environment

**Prerequisites:**
- F01 implemented (the `task-api` binary, its environment variables, `.sqlx/` metadata and `migrations/`)
- Docker Engine with Compose v2 or later on the development machine; Rust stable ≥ 1.100 with `rustfmt` and `clippy`
- Host ports 5432 and 8080 free while exercising the stack
- No new Rust dependencies or application environment variables; `.env.example` gains the two logging variables

### Stage 1: Container Build

**1. Dockerfile** - Create the multi-stage image build described in spec Sections 3 and 4: an offline, dependency-cached builder on the Rust base image and a slim runtime that runs the service as a non-root user with an exec-form entrypoint.

**2. Build Context Filter** - Add the ignore file that keeps build output, version control data, local environment files and unrelated directories out of the build context while preserving the offline query metadata and migrations.

### Stage 2: Compose Stack and Configuration

**3. Compose Definition** - Add the compose file with the database and API services, the named volume, the healthcheck-gated start order, and the stable project, image and shutdown settings listed in spec Section 3.1.

**4. Environment Documentation** - Complete the example environment file with the two logging variables and their defaults so it lists every variable the service reads.

### Stage 3: Checks and Documentation

**5. Static File Checks** - Add the file-shape tests from spec Section 7 so the quality gates catch regressions in the container files without needing Docker.

**6. Documentation Updates** - Document the Docker commands, the host port requirements and the command for running tests against the compose database in the README, and update the project guidance file with the status and conventions.
