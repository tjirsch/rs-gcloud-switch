# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run

```sh
cargo build              # debug build
cargo build --release    # release build
cargo run                # build and run the TUI
cargo run -- list        # run a specific subcommand
cargo check              # quick type-check
cargo clippy             # lint
cargo fmt                # format
```

No test suite exists yet. Verify changes by building (`cargo build`) and manually testing the relevant command.

## Architecture

Rust CLI + TUI app for switching between Google Cloud configurations. Six modules in `src/`:

- **main.rs** — CLI parsing (clap with derive), global settings (`~/.config/gcloud-switch/gcloud-switch.toml`), self-update logic, and the TUI lifecycle. All subcommand dispatch happens here. The `open_file()` function implements editor resolution: configured editor → `$EDITOR` → OS default.
- **app.rs** — TUI state machine. Manages `InputMode` enum (Normal, Edit, AddProfile, ConfirmDelete), profile selection, background auth checking via `mpsc` channels, edit suggestions, and `PendingAction` for deferring operations that require TUI suspension (interactive gcloud auth).
- **ui.rs** — Ratatui rendering. Layout: title bar, profile table, status bar, help line. Handles inline editing with cursor and dropdown suggestion overlays.
- **gcloud.rs** — All gcloud CLI interaction and OAuth2 token validation. Reads `credentials.db` (SQLite, read-only) for tokens, validates via Google's token endpoint, spawns interactive `gcloud auth login` / `gcloud auth application-default login`.
- **store.rs** — Persistent storage under `~/.config/gcloud/gcloud-switch/`. Profiles in TOML, ADC credentials as JSON files per profile.
- **profile.rs** — Data types: `Profile`, `ProfilesFile`, `SyncMode`.
- **sync.rs** — Git-based profile sync using system `git` CLI. Merge strategy: newer `updated_at` timestamp wins per profile.

## Key Design Patterns

- Auth validation runs on **std::thread** (not tokio) because `rusqlite` and `reqwest::blocking` would conflict with a tokio runtime. Auth checks are deduplicated by account email.
- TUI must **suspend** (restore terminal, leave alternate screen) before spawning interactive gcloud commands, then resume. The `PendingAction` enum defers these until the main loop can handle them outside the event handler.
- Two separate config locations: **global settings** in `~/.config/gcloud-switch/gcloud-switch.toml` (update frequency, editor) and **profile data** in `~/.config/gcloud/gcloud-switch/profiles.toml`.
- `BTreeMap` is used for profiles to maintain stable alphabetical ordering.

## Adding CLI Subcommands

1. Add a variant to the `Commands` enum in `main.rs` (clap derive)
2. Add the match arm in `main()`
3. If the command shouldn't trigger update checks, add it to the `!matches!()` guard
4. Update `README.md` with usage examples
