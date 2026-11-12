# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview
A cross-platform desktop binary built in Rust for transferring photos and videos from mobile devices to a PC.

## Build & Verification Commands
Run these checks before completing any task:
- Format: `cargo fmt --all`
- Lint: `cargo clippy --all-targets -- -D warnings`
- Test: `cargo test`
- Fast compile check (during iteration): `cargo check`

## Architecture & Concurrency Rules
- **UI Thread Isolation**: Never perform blocking disk I/O, network requests, or device polling on the main UI thread. Offload all transfers and directory scans to background tasks (e.g., `tokio::spawn` or worker threads).
- **Progress Reporting**: Stream transfer progress, speed, and errors back to the UI via bounded channels (`tokio::sync::mpsc` or framework subscriptions) rather than shared `Arc<Mutex<State>>` locks.
- **UI Framework Rules**:
  - *If using Iced*: Follow The Elm Architecture (State -> Message -> Update -> View). Keep `update` pure and fast; return `Task` (formerly `Command`) for async operations.
  - *If using Slint*: Keep business logic in Rust. Only bind display primitives and callbacks to `.slint` properties, and use `slint::invoke_from_event_loop` when updating UI state from background threads.

## File System & Transfer Safety
- **Data Integrity**: Never delete or overwrite source files on the phone until the destination file is written, flushed, and verified (e.g., matching file size or SHA-256/BLAKE3 hash).
- **Cross-Platform Paths**: Always use `std::path::Path` and `PathBuf`. Never construct file paths using string formatting or hardcoded `/` or `\` separators.
- **Streaming I/O**: Transfer large media files (RAW photos, 4K videos) using buffered streaming (`tokio::io::copy` or chunked reads). Never read entire media files into memory (`fs::read`).

## Rust Coding Conventions
- **Error Handling**: Use `anyhow::Result` for top-level application flow and `thiserror` for structured domain errors in `src/transfer/` (e.g., `DeviceDisconnected`, `InsufficientDiskSpace`). Never use `.unwrap()` or `.expect()` in non-test code.
- **Logging**: Use `tracing` (`info!`, `warn!`, `error!`) for diagnostics. Do not leave `println!` or `dbg!` in production code.
- **Dependencies**: Do not add new crates to `Cargo.toml` without explicit approval.

## Rules for AI Agents
### DO
- Help the human contributor understand code and debug issues
- Suggest improvements that follow existing patterns in the codebase
- Write tests for new and existing functionality

### DON'T
- Generate entire pull requests autonomously
- Open issues based on static analysis without human verification
- Refactor code without prior discussion with maintainers
- Add new dependencies without justification
- Introduce patterns that don't already exist in the codebase without discussion
- Submit code that the human operator cannot explain or modify
- Proactively create documentation or markdown wrap-up files after completing unrelated tasks

Keep replies concise and professional, without emojis, unnecessary affirmations, or explanations of obvious points.