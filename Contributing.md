# Contributing

Thanks for helping improve `herdr-scratch`.

## Development Setup

Requirements:

- Rust stable with edition 2024 support
- Herdr installed for integration testing

Build and test:

```bash
cargo fmt
cargo check
cargo test
```

Link locally in Herdr:

```bash
cargo build --release
herdr plugin link .
target/release/herdr-scratch doctor
```

## Project Principles

- Public APIs talk about scratchpads, not Herdr internals.
- Keep backend details inside `src/herdr.rs`.
- Treat registry handles as soft references and validate before use.
- Keep config and registry versioned.
- Prefer small, reviewable changes.
- Add tests for public config, registry, and response parsing behavior.

## Pull Request Checklist

- `cargo fmt` passes.
- `cargo check` passes.
- `cargo test` passes.
- Public docs are updated when CLI/config/registry behavior changes.
- No user-facing docs expose backend implementation details.

## Reporting Issues

Please include:

- Herdr version
- `herdr-scratch doctor --json` output
- command that failed
- relevant `config.toml`
- whether the scratchpad was newly opened, reused, or stale

Do not include secrets from environment variables or terminal output.
