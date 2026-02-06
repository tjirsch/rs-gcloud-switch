# gcloud-switch

A TUI (Terminal User Interface) tool for managing and switching between multiple Google Cloud configurations. Quickly switch gcloud user credentials and Application Default Credentials (ADC) across different projects and accounts.

## Features

- Interactive TUI for browsing and activating profiles
- Manages both **user credentials** (`gcloud auth`) and **ADC** (`gcloud auth application-default`) per profile
- Auto-detects expired tokens and triggers re-authentication before activation
- Visual auth status indicators (locked/unlocked) per profile
- Import existing gcloud configurations
- CLI subcommands for scripting

## Installation

```sh
cargo install --path .
```

Requires Rust 1.70+ and a working `gcloud` CLI installation.

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

Suggestions include all account emails from existing profiles plus all authenticated accounts from gcloud's credential store.

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

Profiles are stored in `~/.config/gcloud-switch/profiles.toml`:

```toml
[profiles.myprofile]
user_account = "user@example.com"
user_project = "my-project"
adc_account = "user@example.com"
adc_quota_project = "my-project"
```

### Activation

When a profile is activated:

1. **User config**: A gcloud configuration file is written to `~/.config/gcloud/configurations/config_<name>` and set as the active config in `~/.config/gcloud/active_config`
2. **ADC**: The stored ADC JSON is copied to `~/.config/gcloud/application_default_credentials.json`

### Auth Validation

On startup, gcloud-switch reads `~/.config/gcloud/credentials.db` (a SQLite database maintained by gcloud) to look up stored OAuth2 credentials for each profile's account. It then performs a token refresh request to validate whether the credentials are still valid. The result is shown as a lock indicator:

- Unlocked: Token is valid, profile can be activated immediately
- Locked: Token is expired or missing, re-authentication will be triggered on activation

### Re-authentication

When activating a profile with an invalid token, gcloud-switch automatically runs:
- `gcloud auth login --account=<email>` for user credentials
- `gcloud auth application-default login` for ADC credentials

You can also manually trigger re-auth with the `r` key.

## File Locations

| Path | Description |
|------|-------------|
| `~/.config/gcloud-switch/profiles.toml` | Profile definitions |
| `~/.config/gcloud-switch/state.toml` | Active profile state |
| `~/.config/gcloud-switch/adc/<name>.json` | Stored ADC credentials per profile |
| `~/.config/gcloud/credentials.db` | gcloud's OAuth2 credential store (read-only) |
| `~/.config/gcloud/configurations/` | gcloud configuration files (written on activate) |
| `~/.config/gcloud/active_config` | gcloud's active configuration pointer |
| `~/.config/gcloud/application_default_credentials.json` | Active ADC file |

## License

MIT
