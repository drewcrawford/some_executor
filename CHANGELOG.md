# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.2]

### Added

- **Panic support for builtin executors** - You can now set `SOME_EXECUTOR_BUILTIN_SHOULD_PANIC=1` to make the builtin executor panic on task failures. Perfect for debugging when you want your executor to tell you loudly that something went wrong, rather than silently carrying on.

- **Improved WASM32 build configuration** - Fresh cargo config flags for wasm32 targets that make cross-compilation smoother. Your WASM builds just got a little friendlier.

### Changed

- **Documentation refresh** - Clearer docs to help you get up and running faster. We're not saying the old docs were confusing, but... let's just say these are better. Plus some fresh README updates so you know what you're getting into.

- **CI pipeline updates** - Housekeeping on the CI front to keep things running smoothly. Nothing you'll notice, but our build robots are happier.

### Fixed

- **WASM-bindgen compatibility** - Squashed a pesky issue ([wasm-bindgen#4820](https://github.com/wasm-bindgen/wasm-bindgen/issues/4820)) that was making WASM builds grumpy. Your WebAssembly projects should now compile without the drama.

### Housekeeping

- Added `.gitignore` for cleaner repos
- Updated Cargo metadata
- Bumped `test-executors` dev dependency to latest
- Clippy made us tidy up some code—you won't see the difference, but it's there, judging us silently

## [0.6.1] - Previous Release

*Initial tracked release for changelog purposes.*

[Unreleased]: https://github.com/drewcrawford/some_executor/compare/v0.6.2...HEAD
[0.6.2]: https://github.com/drewcrawford/some_executor/compare/v0.6.1...v0.6.2
[0.6.1]: https://github.com/drewcrawford/some_executor/releases/tag/v0.6.1
