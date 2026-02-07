# gcloud-switch

A TUI (Terminal User Interface) tool for managing and switching between multiple Google Cloud configurations. Quickly switch gcloud user credentials and Application Default Credentials (ADC) across different projects and accounts.

## Features

- Interactive TUI for browsing and activating profiles
- Manages both **user credentials** (`gcloud auth`) and **ADC** (`gcloud auth application-default`) per profile
- Auto-detects expired tokens and triggers re-authentication before activation
- Visual auth status indicators (🔑 valid / 🔒 expired) per profile
- Import existing gcloud configurations
- CLI subcommands for scripting

## Installation

```sh
cargo install --path .
```

Requires Rust 1.70+ and a working `gcloud` CLI installation.

## Development

```bash
cargo build                # Debug build
cargo build --release      # Release build
cargo run                  # Build and run the TUI
cargo check                # Quick type-check without building
cargo clippy               # Lint
cargo fmt                  # Format code
```

## Usage

### TUI (default)

```sh
gcloud-switch
```

Opens an interactive table of profiles. Use the keyboard to navigate and activate.

### Key Bindings

| Key | Action |
|-----|--------|
| `Down` | Move selection down |
| `Up` | Move selection up |
| `Left` | Move column left (Both -> User) |
| `Right` | Move column right (User -> ADC) |
| `Enter` | Activate selected profile and quit |
| `Alt+Enter` | Activate selected profile and stay |
| `r` | Re-authenticate selected profile |
| `e` | Edit selected profile in-place |
| `a` / `n` | Add a new profile |
| `d` | Delete selected profile |
| `q` / `Esc` | Quit |

#### Edit Mode

| Key | Action |
|-----|--------|
| Type | Modify the field value directly in the table cell |
| `Down` | Open suggestion dropdown (known accounts or projects) |
| `Up` / `Down` | Navigate suggestions |
| `Enter` | Pick suggestion (if dropdown open) or save and exit |
| `Tab` | Move from account field to project field; save from project |
| `Esc` | Cancel edit without saving |

Suggestions include all account emails from existing profiles plus all authenticated accounts from gcloud's credential store. Project suggestions also include GCP projects accessible by the entered account.

### Column Selection

- **Both** (default): Activates both user config and ADC together
- **User**: Activates only the gcloud user configuration (account + project)
- **ADC**: Activates only the Application Default Credentials

### CLI Subcommands

```sh
# Add a profile
gcloud-switch add myprofile --account user@example.com --project my-project

# Add with separate ADC settings
gcloud-switch add myprofile \
  --account user@example.com \
  --project my-project \
  --adc-account other@example.com \
  --adc-quota-project other-project

# List all profiles
gcloud-switch list

# Switch to a profile (non-interactive)
gcloud-switch switch myprofile

# Import existing gcloud configurations
gcloud-switch import
```

## Data Flow

### Profile Storage

Profiles are stored in `~/.config/gcloud/gcloud-switch/profiles.toml`:

```toml
[profiles.myprofile]
user_account = "user@example.com"
user_project = "my-project"
adc_account = "user@example.com"
adc_quota_project = "my-project"
```

### Activation

When a profile is activated:

1. **User config**: A gcloud configuration is created (if needed) and activated via `gcloud config configurations activate`, then account and project are set via `gcloud config set`
2. **ADC**: The stored ADC JSON is copied to `~/.config/gcloud/application_default_credentials.json`

### Auth Validation

On startup, gcloud-switch reads `~/.config/gcloud/credentials.db` (a SQLite database maintained by gcloud) to look up stored OAuth2 credentials for each profile's account. It then performs a token refresh request to validate whether the credentials are still valid. The result is shown as a lock indicator:

- 🔑 Token is valid, profile can be activated immediately
- 🔒 Token is expired or missing, re-authentication will be triggered on activation

### Re-authentication

When activating a profile with an invalid token, gcloud-switch automatically runs:
- `gcloud auth login --account=<email>` for user credentials
- `gcloud auth application-default login` for ADC credentials

You can also manually trigger re-auth with the `r` key.

## File Locations

| Path | Description |
|------|-------------|
| `~/.config/gcloud/gcloud-switch/profiles.toml` | Profile definitions |
| `~/.config/gcloud/gcloud-switch/state.toml` | Active profile state |
| `~/.config/gcloud/gcloud-switch/adc/<name>.json` | Stored ADC credentials per profile |
| `~/.config/gcloud/credentials.db` | gcloud's OAuth2 credential store (read-only) |
| `~/.config/gcloud/configurations/` | gcloud configuration files (written on activate) |
| `~/.config/gcloud/active_config` | gcloud's active configuration pointer |
| `~/.config/gcloud/application_default_credentials.json` | Active ADC file |

## Architecture

Six modules with clear separation:

- **main.rs** — CLI parsing (clap) and TUI lifecycle. Subcommands: `add`, `list`, `switch`, `import`, or no subcommand for interactive TUI. Handles TUI suspend/resume when spawning interactive gcloud auth commands.
- **app.rs** — Core state machine. Manages `InputMode` (Normal, Edit, AddProfile, ConfirmDelete), profile selection, background auth checking, edit suggestions, and pending actions. The `Column` enum controls whether activation targets both user+ADC, user-only, or ADC-only credentials.
- **ui.rs** — Ratatui rendering. Layout is 4 rows: title, table, status bar, help line. Renders inline editing with cursor positioning and dropdown suggestion overlays.
- **gcloud.rs** — All gcloud CLI and OAuth2 integration. Manages configurations via gcloud CLI commands, queries `credentials.db` (SQLite, read-only) for OAuth tokens, validates tokens via Google's token endpoint, and spawns `gcloud auth login` / `gcloud auth application-default login`.
- **store.rs** — Persistent storage in `~/.config/gcloud/gcloud-switch/`. Profiles stored as TOML, ADC credentials as JSON files per profile.
- **profile.rs** — Data structures: `Profile` (user_account, user_project, adc_account, adc_quota_project), `ProfilesFile`, `StateFile`.

### Key Design Decisions

- Auth validation runs on background threads (not tokio tasks) because `rusqlite` and `reqwest::blocking` would conflict with the tokio runtime. Auth checks are deduplicated by account.
- TUI must suspend (restore terminal, drop alternate screen) before spawning interactive gcloud commands, then resume after.
- `PendingAction` enum defers actions that require TUI suspension until the main loop can handle them outside the event handler.
- Profile activation uses gcloud CLI (`gcloud config configurations activate`, `gcloud config set`) to ensure gcloud's internal state stays consistent. ADC file copy is the only direct file operation (no gcloud CLI equivalent exists).

### Dependencies

Key crates: `ratatui` + `crossterm` (TUI), `clap` (CLI), `tokio` (async runtime), `reqwest` (HTTP for token validation), `rusqlite` with bundled SQLite (credentials.db access), `serde` + `toml` + `serde_json` (serialization), `anyhow` (error handling).

## License

MIT
