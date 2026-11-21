# syntax=docker/dockerfile:1

# ---- Builder: compiles the task-api binary offline (no database needed) ----
FROM rust:1.100 AS builder
ENV SQLX_OFFLINE=true
WORKDIR /app

# Dependency layer: build with stub sources so it is cached until the
# manifests or lockfile change.
COPY Cargo.toml Cargo.lock build.rs ./
RUN mkdir -p src migrations \
    && echo 'fn main() {}' > src/main.rs \
    && touch src/lib.rs \
    && cargo build --release --locked \
    && rm -rf src target/release/deps/task_api* target/release/deps/libtask_api* \
       target/release/.fingerprint/task-api*

# Application layer.
COPY src ./src
COPY .sqlx ./.sqlx
COPY migrations ./migrations
RUN touch src/main.rs src/lib.rs \
    && cargo build --release --locked

# ---- Runtime: slim, non-root ----
FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin app
COPY --from=builder /app/target/release/task-api /usr/local/bin/task-api
ENV HOST=0.0.0.0 \
    PORT=8080
USER app
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/task-api"]
