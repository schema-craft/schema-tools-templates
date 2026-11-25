# Schema Tools Templates

This repository contains Tera templates used by [schema-tools](https://github.com/kstasik/schema-tools) to generate Rust code from OpenAPI / JSON Schema specifications.

## Layout

- `rust/` — Tera v2 templates
  - `rust/_common/` — shared components (`macros.j2`, `models.j2`, …)
  - `rust/client/` — reqwest client templates
  - `rust/server-axum/` — axum server templates
  - `rust/server-actix/` — actix-web server templates
  - `rust/rabbitmq/` and `rust/rabbitmq-tokio/` — RabbitMQ message templates
- `tests/` — sample Rust applications that exercise the templates
  - `tests/rust` — actix-web + RabbitMQ
  - `tests/rust-axum` — axum server + client
  - `tests/rust-axum-relaxed` — axum server + client with relaxed validation
  - `tests/rust-tokio-rmq` — `rabbitmq-tokio` producer/consumer integration tests (needs RabbitMQ: `docker run -d -p 5672:5672 rabbitmq:4`, or set `AMQP_URI`)

## Running the tests

Each test crate is a normal Rust project. The generated code is produced by `make codegen` and then checked/built with Cargo.

```bash
# Generate code for one of the sample applications
cd tests/rust-axum-relaxed
make codegen

# Verify the generated code compiles
cargo check

# Run the sample tests
cargo test
```

The same workflow is repeated for the other crates:

```bash
cd tests/rust-axum        && make codegen && cargo check && cargo test
cd tests/rust             && make codegen && cargo check && cargo test
cd tests/rust-tokio-rmq   && make codegen && cargo check && cargo test   # needs RabbitMQ
```

CI runs exactly this sequence — see `.github/workflows/test.yaml` and `.github/workflows/build.yaml`.

### What is generated vs. hand-written?

`make codegen` overwrites files under `src/api/`, `src/client/`, `src/rabbitmq/`, etc. These directories are generated and should be treated as read-only.

`src/main.rs` in each test crate is **not** generated. It contains hand-written example handlers and integration tests that consume the generated API. When a template change alters the generated API surface — for example, renaming error-response models or changing how response headers are wrapped — `src/main.rs` must be updated manually to match the new generated types.

### Current status after the Tera v2 migration

- `make codegen` succeeds for all three test crates.
- The generated library code compiles.
- `cargo test` passes for all three test crates.

The hand-written `src/main.rs` examples were updated to use the newly generated type names. The `test_locations_create_v1` test round-trips the `Simple` variant of the discriminated `oneOf` (fixed in schematools v1.1.0, which no longer duplicates the `testField` key).
