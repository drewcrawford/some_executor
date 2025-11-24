# Repository Guidelines

## Project Structure & Module Organization
- `src/`: core crate code. `context/` handles task context and task-local utilities; `task/` owns spawn/config/types; `sys/` provides platform-specific shims for stdlib vs wasm; `*_executor.rs` files expose global/current/thread executors; `dyn_*` files cover object-safe adapters.
- `tests/`: integration tests (e.g., `tests/static_brute.rs`, `tests/executor_yield_test.rs`). Use the `test_executors` dev dependency for harnesses.
- `examples/`: runnable snippets showing API usage.
- `art/`: branding assets; not needed for builds.
- `Cargo.toml`: crate metadata (edition 2024) and target-specific deps, especially for wasm.

## Build, Test, and Development Commands
- `cargo check` / `cargo build`: fast compile check or full build for the current target.
- `cargo test` or `cargo test -- --nocapture`: run host tests; the latter surfaces logging.
- `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER="wasm-bindgen-test-runner" cargo +nightly test --target wasm32-unknown-unknown`: run wasm tests in a browser runner.
- `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER="wasm-bindgen-test-runner" RUSTFLAGS='-C target-feature=+atomics,+bulk-memory,+mutable-globals' cargo +nightly test --target wasm32-unknown-unknown -Z build-std=std,panic_abort`: wasm tests with atomics.
- `cargo fmt` / `cargo fmt -- --check`: format or verify formatting.
- `cargo clippy --all-targets --all-features`: lint; address warnings or justify deviations.

## Coding Style & Naming Conventions
- Rust 2024 edition with `rustfmt` defaults (4-space indent, trailing commas encouraged). Run `cargo fmt` before sending patches.
- Prefer `snake_case` for modules/functions, `CamelCase` for types/traits, `SCREAMING_SNAKE_CASE` for constants. Task-local identifiers match existing names (e.g., `TASK_ID`, `TASK_PRIORITY`).
- Keep APIs object-safe where required and reuse existing type aliases for erased futures/observers to avoid type bloat.
- Document behavior with `///` doc comments, focusing on invariants and executor semantics rather than restating signatures.

## Testing Guidelines
- Host tests use standard `#[test]` plus conditional wasm attributes; integration suites live in `tests/`. Name new tests after behavior (`*_yields`, `*_cancels`) for clarity.
- Use `test_executors` helpers for lightweight executor instances in unit/integration tests.
- For wasm, ensure `wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser)` is present when adding new suites; keep async tests non-blocking.
- No formal coverage gate, but add focused tests when changing spawn semantics, task locals, or cancellation flows.

## Commit & Pull Request Guidelines
- Commit messages: short, imperative, and scoped (e.g., `Tighten poll scheduling`, `Document wasm runner`). Include rationale when touching executor semantics or public traits.
- Before opening a PR: run `cargo fmt`, `cargo clippy --all-targets`, and the relevant `cargo test` (host and wasm when applicable). Note any skipped targets and why.
- PR description should link related issues, summarize behavior changes, and call out API/semver impacts. Include logs or screenshots only if modifying documentation assets.

## Platform Notes (WASM & Concurrency)
- wasm32 is a first-class target; use the `sys/wasm.rs` implementations and avoid threading APIs unavailable on wasm.
- When adding timeouts or sleeps, prefer existing abstractions in `sys` to stay compatible with `wasm_thread` and `web-time`.
