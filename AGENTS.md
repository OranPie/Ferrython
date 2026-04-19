# Repository Guidelines

Ferrython is a Rust implementation of the Python 3.8 interpreter, organized as a Cargo workspace. This guide orients contributors (human or agent) to the layout, workflows, and conventions used here.

## Project Structure & Module Organization

- `crates/` — workspace members, one per subsystem. Key crates: `ferrython-parser`, `ferrython-compiler`, `ferrython-bytecode`, `ferrython-vm`, `ferrython-core`, `ferrython-gc`, `ferrython-stdlib`, `ferrython-cli` (binary entry point).
- `stdlib/` — Python-side standard library sources shipped with the interpreter.
- `tests/` — integration tests and fixtures. `tests/fixtures/` holds `.py` scenarios; `tests/benchmarks/bench_suite.py` drives perf runs.
- `target/` — Cargo build output (ignored). `Cargo.toml`/`Cargo.lock` define the workspace; `rustfmt.toml` pins formatting.
- Top-level notes: `README.md`, `ARCHITECTURE-AUDIT.md`, `LIMITATIONS.md`, `ferrython-gaps.md`.

## Build, Test, and Development Commands

- `cargo build --release` — optimized build of the whole workspace (alias: `make release`).
- `cargo run --release --bin ferrython -- script.py` — run a Python script through the interpreter; omit args for the REPL.
- `cargo test` — run all Rust unit and integration tests across crates.
- `cargo test -p ferrython-vm` — scope tests to a single crate during focused work.
- `cargo fmt --all` / `cargo clippy --all-targets -- -D warnings` — format and lint before pushing.
- `make bench` — benchmark CPython vs. the release binary via `tests/benchmarks/bench_suite.py`.
- `make pgo` — full PGO build (instrument, collect, rebuild); requires `llvm-profdata`.

## Coding Style & Naming Conventions

- Rust 2021, MSRV 1.75. Format with `rustfmt` (`max_width = 100`, field-init + try shorthand).
- Use `snake_case` for functions/modules, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for consts. Crates follow the `ferrython-<area>` pattern.
- Keep comments sparse and meaningful; prefer doc comments (`///`) on public APIs. Avoid `unsafe` unless locally justified.

## Testing Guidelines

- Unit tests live alongside source (`#[cfg(test)] mod tests`). Integration-style Python programs go under `tests/fixtures/` and are driven from Rust or via the CLI.
- Name files `test_<feature>.py` or `test_<feature>.rs`; name cases after the behavior they cover.
- Run `cargo test` before every commit; add fixtures when fixing interpreter bugs and reference them in the commit message.

## Commit & Pull Request Guidelines

- Commit subjects are short, imperative, and often prefixed with a scope such as `perf:`, `fix:`, or a subsystem label (e.g. `Optimize str.join: ...`). Keep them under ~72 chars.
- Each PR should describe motivation, summarize changes, list verification (`cargo test`, `make bench` deltas when perf-related), and link issues. Include before/after numbers for performance work.
