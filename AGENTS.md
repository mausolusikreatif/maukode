# Project: maukode

Rust multi-crate workspace (maukode)

## Tech Stack
- **Language**: Rust
- **Build System**: Cargo
- **Package Manager**: cargo
- **Key Frameworks / Libraries**: tokio, axum, serde, ratatui

## Commands

### Build
```bash
cargo build
```

### Test
```bash
cargo test --workspace
```

### Lint & Format
```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all
```

## Code Style & Conventions
- **Idiomatic Error Handling**: Use `Result<T, E>` with `thiserror` or `anyhow`; avoid `unwrap()` or `expect()` in production paths.
- **Formatting**: Follow standard `rustfmt` guidelines; keep line length <= 100 where reasonable.
- **Async Runtime**: Prefer `tokio` for async tasks and non-blocking I/O.
- **Clippy**: Ensure code passes `cargo clippy --all-targets` without warnings.

## Key Directories
- `crates/`: Core project components

## Agent Guidelines
- **Inspect Before Modifying**: Read existing code to understand architecture and conventions before editing.
- **Surgical Edits**: Use surgical search-and-replace rather than overwriting entire files whenever possible.
- **Verify Changes**: Always execute the test suite (`cargo test --workspace`) before declaring a task complete.
- **Documentation Integrity**: Keep comments, docstrings, and documentation up to date with code changes.
