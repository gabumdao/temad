# temad — Dynamic Theme and Wallpaper Manager

A lightweight, highly efficient, and fully configurable **dynamic wallpaper and theme manager** written in Rust. Designed as a drop-in replacement for fragile bash script pipelines, `temad` provides a compiled, single-binary daemon that handles wallpaper slideshows, color extraction, and desktop theming through a simple TOML configuration file.

Built for **Wayland** environments — specifically targeting compositors like [Hyprland](https://hyprland.org/) and [Sway](https://swaywm.org/).

## Project Docs

- [Roadmap](docs/ROADMAP.md)

---

## Features

- **Compiled & Lightweight** — Single binary, minimal CPU/RAM footprint. No shell subprocesses spawned in loops.
- **Configuration-Driven** — No hardcoded paths or commands. Everything lives in `~/.config/temad/config.toml`.
- **Multiple Color Engines** — Supports [wallust](https://codeberg.org/explosion-mental/wallust) (default) and [matugen](https://github.com/InioX/matugen), or disable color extraction entirely.
- **Multiple Wallpaper Engines** — Supports [swww](https://github.com/LGFae/swww) (default), [hyprpaper](https://github.com/hyprwm/hyprpaper), and [wbg](https://codeberg.org/dnkl/wbg).
- **Post-Update Hooks** — Execute arbitrary shell commands after each wallpaper change (e.g., update Hyprland border colors, restart notification daemons).
- **Wallust Variable Sourcing** — When using `wallust`, hook commands automatically have access to color variables like `$color2`, `$background`, `$foreground`, etc.
- **Slideshow Daemon** — Background daemon with configurable interval, random or sequential image cycling, and no-repeat guarantee.
- **One-Shot Mode** — Immediately set a specific wallpaper and apply the full theme pipeline.
- **Status Inspection** — Print the effective configuration, wallpaper directory health, state file, and current wallpaper summary without changing anything.
- **Doctor Diagnostics** — Verify config loading, backend binaries, wallpaper directory readiness, and daemon health without mutating your theme state.
- **Next/Previous Wallpaper Mode** — Move through wallpapers on demand using your configured random/sequential behavior with a tiny bounded history state.

---

## Prerequisites & Dependencies

### Required

| Dependency | Purpose |
|---|---|
| **Rust toolchain** | Build the project (`rustup` recommended) |
| **Wayland compositor** | Hyprland, Sway, or any wlroots-based compositor |

### Color Engines (choose one)

| Engine | Install | Notes |
|---|---|---|
| [wallust](https://codeberg.org/explosion-mental/wallust) | `cargo install wallust` | **Default.** Generates color schemes and exports shell variables to `~/.cache/wallust/colors.sh` |
| [matugen](https://github.com/InioX/matugen) | `cargo install matugen` | Material You color generation |
| *none* | — | Disable color extraction entirely |

### Wallpaper Engines (choose one)

| Engine | Install | Notes |
|---|---|---|
| [swww](https://github.com/LGFae/swww) | `cargo install swww` | **Default.** Efficient animated wallpaper daemon. `temad` auto-starts `swww-daemon` if not running |
| [hyprpaper](https://github.com/hyprwm/hyprpaper) | Via your package manager | Hyprland's native wallpaper utility |
| [wbg](https://codeberg.org/dnkl/wbg) | Via your package manager | Minimal Wayland background setter |

### Optional / Recommended

| Tool | Purpose |
|---|---|
| [swaync](https://github.com/ErikReider/SwayNotificationCenter) | Notification center — reload styles via hooks |
| [waybar](https://github.com/Alexays/Waybar) | Status bar — signal reload via hooks |
| [hyprctl](https://wiki.hyprland.org/Configuring/Using-hyprctl/) | Hyprland IPC — update border colors, reload config |

---

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/benitezgabriel/temad.git
cd temad

# Build the release binary
cargo build --release

# Install to your PATH
cp target/release/temad ~/.local/bin/

# Verify
temad --version
```

> **Tip:** Ensure `~/.local/bin` is in your `$PATH`. Add `export PATH="$HOME/.local/bin:$PATH"` to your shell profile if needed.

### One-liner with cargo

```bash
cargo install --path .
```

### Shell Completions

Generate completions to stdout and install them into your shell's completion directory:

```bash
# Bash
mkdir -p ~/.local/share/bash-completion/completions
temad completions bash > ~/.local/share/bash-completion/completions/temad

# Zsh
mkdir -p ~/.zfunc
temad completions zsh > ~/.zfunc/_temad

# Fish
mkdir -p ~/.config/fish/completions
temad completions fish > ~/.config/fish/completions/temad.fish
```

Reload your shell or start a new session after installing completions.

---

## Configuration

`temad` reads its configuration from `~/.config/temad/config.toml`. If the file does not exist, sensible defaults are used.

### Create the Config Directory

```bash
mkdir -p ~/.config/temad
```

### Complete Example (`config.toml`)

```toml
# =============================================================================
# temad — Dynamic Theme & Wallpaper Manager Configuration
# =============================================================================

[slideshow]
# Directory containing your wallpaper images.
# Supports ~ expansion.
directory = "~/Pictures/Wallpapers"

# Seconds between wallpaper changes (default: 300 = 5 minutes).
interval_seconds = 300

# Randomize image selection order (default: true).
# When false, images cycle alphabetically.
randomize = true

# Maximum number of wallpaper history entries to persist (default: 32).
history_limit = 32

# Recursively scan subdirectories for supported wallpaper files (default: false).
recursive = false

[backends]
# Color extraction backend.
# Supported: "wallust" (default), "matugen", "none"
colors = "wallust"

# Wallpaper setting backend.
# Supported: "swww" (default), "hyprpaper", "wbg"
wallpaper = "swww"

[wallpaper_targets]
# Optional list of monitor/output names to target with the selected wallpaper.
# Leave empty to target all outputs supported by the backend.
outputs = []

[swww_options]
# These options are passed to `swww img` when the wallpaper backend is "swww".
# See `swww img --help` for all available transition types.
transition_type = "random"
transition_step = 100
transition_fps = 60
transition_duration = 2

[commands]
# Maximum number of seconds to allow external commands to run (default: 30).
timeout_seconds = 30

[hooks]
# Shell commands executed AFTER colors and wallpaper are applied.
# Each command runs via `sh -c "..."`.
#
# IMPORTANT: When using wallust as the color backend, temad automatically
# sources ~/.cache/wallust/colors.sh BEFORE each hook, so you can use
# shell variables like $color2, $background, $foreground, etc.
#
# The `:1` syntax (e.g., ${color2:1}) strips the leading '#' from the
# hex color — this is a wallust convention for use with hyprctl rgba().
post_update = [
    "hyprctl keyword general:col.active_border 'rgba(${color2:1}ff) rgba(${color4:1}ff) 45deg'",
    "hyprctl keyword general:col.inactive_border 'rgba(${background:1}aa)'",
    "swaync-client -rs",
    "pkill -RTMIN+8 waybar"
]
```

### Section Reference

#### `[slideshow]`

| Key | Type | Default | Description |
|---|---|---|---|
| `directory` | String | `~/Pictures/Wallpapers` | Path to your wallpaper directory. Supports `~` expansion. |
| `interval_seconds` | Integer | `300` | Seconds between wallpaper changes in daemon mode. |
| `randomize` | Boolean | `true` | Random selection (`true`) or alphabetical cycling (`false`). |
| `history_limit` | Integer | `32` | Maximum number of persisted wallpaper history entries. |
| `recursive` | Boolean | `false` | Recursively scan subdirectories for supported wallpapers. |

#### `[backends]`

| Key | Type | Default | Options |
|---|---|---|---|
| `colors` | String | `wallust` | `wallust`, `matugen`, `none` |
| `wallpaper` | String | `swww` | `swww`, `hyprpaper`, `wbg` |

#### `[wallpaper_targets]`

| Key | Type | Default | Description |
|---|---|---|---|
| `outputs` | String array | `[]` | Optional monitor/output names to target. `swww` and `hyprpaper` support this; `wbg` does not. |

#### `[commands]`

| Key | Type | Default | Description |
|---|---|---|---|
| `timeout_seconds` | Integer | `30` | Maximum runtime allowed for external commands before `temad` aborts them. |

#### `[swww_options]`

Only used when `wallpaper = "swww"`. These are passed directly to `swww img`.

| Key | Type | Default | Description |
|---|---|---|---|
| `transition_type` | String | `random` | Transition animation type (see `swww img --help`). |
| `transition_step` | Integer | `100` | Speed of the transition (higher = faster). |
| `transition_fps` | Integer | `60` | Frames per second for the transition animation. |
| `transition_duration` | Integer | unset | Optional transition duration passed to `swww img`. |

#### `[hooks]`

The `post_update` array contains shell commands that execute **after** every wallpaper and color update. This is the primary mechanism for propagating color changes to your desktop environment.

**How color variable sourcing works:**

When `colors = "wallust"`, each hook is wrapped as:

```bash
sh -c 'if [ -f "/home/user/.cache/wallust/colors.sh" ]; then . "/home/user/.cache/wallust/colors.sh"; fi; <your_hook_command>'
```

This means variables exported by wallust — such as `$background`, `$foreground`, `$cursor`, `$color0` through `$color15` — are available inside your hook commands. The `${variable:1}` syntax is standard shell parameter expansion that strips the first character (the `#` prefix from hex colors), which is required by `hyprctl`'s `rgba()` format.

When using `matugen` or `none` as the color backend, hooks run as-is without any sourcing.

---

## Usage

### Start the Slideshow Daemon

```bash
temad daemon
```

This starts the background loop that:
1. Scans the configured wallpaper directory for image files (`.jpg`, `.jpeg`, `.png`, `.webp`, `.bmp`, `.gif`).
2. Picks an image (random or sequential, per config).
3. Extracts colors using the configured color engine.
4. Sets the wallpaper using the configured wallpaper engine.
5. Runs all post-update hooks.
6. Sleeps for `interval_seconds`, then repeats.

> **Tip:** Add `exec temad daemon &` to your Hyprland config (`hyprland.conf`) or Sway config to auto-start on login.

### Reload a Running Daemon

```bash
temad reload
```

This asks the running daemon for the resolved config path to reload its configuration and persisted wallpaper history without restarting the user session. A successful reload resets the slideshow countdown using the newly loaded interval.

If no daemon is listening for that config path, `temad reload` exits with an error instead of silently starting a new daemon.

If you replaced the `temad` binary after Hyprland started it with `exec-once`, the old daemon keeps running until you restart it. In that case `temad reload` cannot talk to the old process because it never created the control socket.

### Pause and Resume the Slideshow

```bash
temad pause
temad play
```

`temad pause` freezes slideshow progression inside the running daemon without changing the current wallpaper. `temad play` resumes progression from the frozen remaining time instead of resetting the full interval.

### Update the Active Interval

```bash
temad interval 45
```

This updates the active slideshow interval inside the running daemon without rewriting `config.toml`. If the slideshow is running, the countdown resets from now using the new interval. If it is paused, the frozen remaining time is clamped to the new interval and resumes from there when you run `temad play`.

### Set a Specific Wallpaper

```bash
temad set ~/Pictures/Wallpapers/sunset.jpg
```

This immediately runs the full pipeline (color extraction → wallpaper → hooks) for the specified image and exits. The path must exist, point to a file, and use a supported image extension (`.jpg`, `.jpeg`, `.png`, `.webp`, `.bmp`, `.gif`).

### Inspect Current Status

```bash
temad status
```

This prints the effective config path, whether defaults are in use, wallpaper directory health, slideshow mode, configured backends, `swww-daemon` availability when relevant, and a summary of the persisted history state including the current wallpaper when known.

When a reload-capable daemon is running, `temad status` also shows the time remaining until the next scheduled wallpaper change. If no daemon is listening, that field is reported as `n/a`.

When pause/play support is available, `temad status` also reports the slideshow state as `running` or `paused`.

When interval control is available, `temad status` also shows `Active interval seconds`, which may differ from the configured interval until the daemon is restarted or reloaded.

### Run Diagnostics

```bash
temad doctor
```

This runs non-mutating diagnostics for the resolved config path, wallpaper directory, selected backend binaries, and `swww-daemon` runtime state. It reports `OK`, `WARN`, and `FAIL` lines and exits non-zero only when required checks fail.

When `wallpaper_targets.outputs` is configured, `temad` applies the same selected image only to those outputs when the backend supports it.

### Preview Without Changing Anything

```bash
temad --dry-run set ~/Pictures/Wallpapers/sunset.jpg
temad --dry-run next
temad --dry-run prev
temad --dry-run daemon
temad --dry-run reload
```

This resolves the wallpaper choice and prints the external commands `temad` would run, but does not apply wallpapers, execute hooks, or persist history updates. In `daemon` mode, dry-run previews a single cycle and exits.

For `reload`, dry-run validates the config and prints the control socket path it would contact, but does not connect to the daemon.

If `reload` reports that no reload-capable daemon is listening, verify the running process and restart the daemon from the current binary:

```bash
pgrep -af temad
readlink -f /proc/$(pgrep -xo temad)/exe
ls /run/user/$(id -u)/temad
pkill -x temad
temad daemon
```

### Inspect Persisted History

```bash
temad history
```

This prints the bounded wallpaper history currently persisted in XDG state, including the current entry marker when available. It is read-only and does not apply or rewrite wallpapers.

The state file (`$XDG_STATE_HOME/temad/last_image`, normally `~/.local/state/temad/last_image`) stores a zero-based cursor on its first line, followed by timestamped history entries in the original `@<unix-seconds><TAB><image-path>` format. Timestamps represent when an image was **added to history** after a successful apply; navigating with `prev` / `next` retains each entry's original timestamp. `set` and automatic slideshow changes create timestamped entries as well.

Older plain-path entries remain readable. If they have already lost their timestamps, Temad leaves those timestamps unknown rather than inventing dates; newly recorded entries always receive a real Unix timestamp. No automatic cleanup or deletion of existing history takes place.

### Bootstrap a Starter Config

```bash
temad init
```

This creates a starter `config.toml` at the resolved config path if one does not already exist. Re-running it is safe: existing config files are left untouched.

### Apply the Next Wallpaper

```bash
temad next
```

This selects and applies one wallpaper from the configured directory using `[slideshow].randomize`. In sequential mode it advances from the last successfully applied image; in random mode it avoids immediately repeating the current image when possible. `temad` persists a tiny bounded history in XDG state so `next` and `prev` stay consistent across runs without heavy disk writes.

History persistence is now written atomically and guarded by a lock file so concurrent `set`/`next`/`prev`/`daemon` updates do not leave behind truncated or interleaved state.

### Apply the Previous Wallpaper

```bash
temad prev
```

This selects and applies the previous wallpaper from the configured directory using `[slideshow].randomize`. In sequential mode it moves backward through the sorted wallpaper list, wrapping from the first image to the last. In random mode it walks backward through the bounded wallpaper history that `temad` persists, so it returns to the real previously applied wallpaper while history is available.

### CLI Options

```
Usage: temad [OPTIONS] <COMMAND>

Commands:
  daemon       Start the background slideshow daemon
  reload       Ask a running daemon to reload config and state
  pause        Pause slideshow progression in the running daemon
  play         Resume slideshow progression in the running daemon
  interval     Update the active slideshow interval in the running daemon
  status       Show the effective configuration and current wallpaper state
  doctor       Run non-mutating diagnostics for config and runtime dependencies
  history      Inspect the persisted wallpaper history
  init         Create a starter config file if one does not exist
  set          Immediately set a specific wallpaper and extract colors
  next         Apply the next wallpaper from the configured directory
  prev         Apply the previous wallpaper from the configured directory
  completions  Generate shell completion scripts
  help         Print this message or the help of the given subcommand(s)

Options:
  -c, --config <CONFIG>  Path to a custom configuration file
      --dry-run          Preview actions without changing wallpaper state
  -v, --verbose...       Set the verbosity level (-v, -vv, -vvv)
  -h, --help             Print help
  -V, --version          Print version
```

#### Verbosity Levels

| Flag | Level | What you see |
|---|---|---|
| *(none)* | `info` | Wallpaper changes, hook execution |
| `-v` | `debug` | Command arguments, sleep timers |
| `-vv` | `trace` | Everything including internal state |

You can also control logging via the `RUST_LOG` environment variable:

```bash
RUST_LOG=temad=debug temad daemon
```

---

## Autostart Examples

### Hyprland (`~/.config/hypr/hyprland.conf`)

```conf
exec-once = temad daemon
```

### Sway (`~/.config/sway/config`)

```conf
exec temad daemon
```

### Systemd User Service

```ini
# ~/.config/systemd/user/temad.service
[Unit]
Description=temad Dynamic Theme Manager
After=graphical-session.target

[Service]
ExecStart=%h/.local/bin/temad daemon
Restart=on-failure
RestartSec=5

[Install]
WantedBy=graphical-session.target
```

```bash
systemctl --user enable --now temad.service
```

---

## Project Structure

```
src/
├── main.rs        # Entry point, CLI dispatch, logging init
├── cli.rs         # clap-based CLI definition
├── config.rs      # TOML configuration parsing & defaults
├── doctor.rs      # Non-mutating config and dependency diagnostics
├── engine.rs      # Color/wallpaper backends, hooks, CommandRunner trait
├── history.rs     # Read-only persisted history reporting
├── init.rs        # Starter config bootstrap
├── status.rs      # Non-mutating status reporting
├── slideshow.rs   # Facade for slideshow-related APIs
└── slideshow/
    ├── discovery.rs     # Wallpaper discovery and recursive scanning
    ├── history_state.rs # Persisted history state and locking
    ├── runtime.rs       # Daemon, next, and prev orchestration
    └── selector.rs      # Image selection logic
```

---

## Testing

The project includes **134 unit and integration tests** covering:

- Configuration parsing, defaults, and edge cases
- Multi-monitor backend targeting across supported wallpaper engines
- Path expansion (`~` to home directory)
- Diagnostic reporting for config, dependencies, and daemon health
- History inspection and config bootstrap commands
- Image discovery and filtering
- Lock-safe and atomic wallpaper history persistence
- Status reporting and persisted state inspection
- Sequential and random image selection (no-repeat guarantee)
- Command generation for all backend combinations
- Hook formatting and `colors.sh` sourcing
- Error propagation and resilience

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run a specific test module
cargo test config::tests
cargo test engine::tests
cargo test slideshow::tests
```

---

## License

MIT
