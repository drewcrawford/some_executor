# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Panic support for builtin executors** - You can now set `SOME_EXECUTOR_BUILTIN_SHOULD_PANIC=1` to make the builtin executor panic on task failures. Perfect for debugging when you want your executor to tell you loudly that something went wrong, rather than silently carrying on.

- **Improved WASM32 build configuration** - Fresh cargo config flags for wasm32 targets that make cross-compilation smoother. Your WASM builds just got a little friendlier.

### Changed

- **Documentation refresh** - Clearer docs to help you get up and running faster. We're not saying the old docs were confusing, but... let's just say these are better.

- **CI pipeline updates** - Housekeeping on the CI front to keep things running smoothly. Nothing you'll notice, but our build robots are happier.

### Housekeeping

- Added `.gitignore` for cleaner repos
- Updated Cargo metadata

## [0.6.1] - Previous Release

*Initial tracked release for changelog purposes.*

[Unreleased]: https://github.com/drewcrawford/some_executor/compare/v0.6.1...HEAD
[0.6.1]: https://github.com/drewcrawford/some_executor/releases/tag/v0.6.1
