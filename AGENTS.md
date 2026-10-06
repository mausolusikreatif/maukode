# Project: maukode

Rust multi-crate workspace (maukode)

## Tech Stack
- **Language**: Rust
- **Build System**: Cargo
- **Package Manager**: cargo
- **Key Frameworks / Libraries**: tokio, axum, serde, ratatui

> [!IMPORTANT]
> **NO LOCAL BUILDS OR TESTS**:
> Agents running in this workspace MUST NEVER execute `cargo build`, `cargo test`, `cargo clippy`, `cargo check`, `cargo run`, `npm test`, or any compilation/test commands on the local machine.
> Running builds and test suites locally burns CPU and severely bottlenecks the system, especially when running multiple agents in parallel.
> All compilation, testing, clippy, and CI verification MUST be offloaded to **GitHub CI**.

## CI & Reference Commands (GitHub CI Only)

> [!CAUTION]
> **DO NOT EXECUTE THESE COMMANDS LOCALLY.**
> These commands are configured for GitHub Actions (`.github/workflows/ci.yml`). Do not run them on the local host.

### Test (GitHub CI)
```bash
cargo test --workspace
```

### Build (GitHub CI)
```bash
cargo build
```

### Lint (GitHub CI)
```bash
cargo clippy --all-targets --all-features -- -D warnings
```

### Format (GitHub CI / Non-compiling)
```bash
cargo fmt --all
```

## Code Style & Conventions
- **Idiomatic Error Handling**: Use `Result<T, E>` with `thiserror` or `anyhow`; avoid `unwrap()` or `expect()` in production paths.
- **Formatting**: Follow standard `rustfmt` guidelines; keep line length <= 100 where reasonable.
- **Async Runtime**: Prefer `tokio` for async tasks and non-blocking I/O.
- **Clippy**: Ensure code passes `cargo clippy --all-targets` without warnings (verified in CI).

## Key Directories
- `crates/`: Core project components

## Agent Guidelines
- **Strictly No Local Builds or Tests**: NEVER run `cargo test`, `cargo build`, `cargo clippy`, `cargo check`, `npm test`, or any test runners/compilers locally. Always rely on GitHub CI for build, lint, and test execution.
- **Static & Surgical Verification**: Verify changes through careful code inspection, logic analysis, type-signature tracing, and surgical diffs. Do NOT execute local test suites or compilation targets before declaring a task complete.
- **Inspect Before Modifying**: Read existing code to understand architecture and conventions before editing.
- **Surgical Edits**: Use surgical search-and-replace rather than overwriting entire files whenever possible.
- **Documentation Integrity**: Keep comments, docstrings, and documentation up to date with code changes.

