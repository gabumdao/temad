# temad — Dynamic Theme and Wallpaper Manager

A lightweight, highly efficient, and fully configurable **dynamic wallpaper and theme manager** written in Rust. Designed as a drop-in replacement for fragile bash script pipelines, `temad` provides a compiled, single-binary daemon that handles wallpaper slideshows, color extraction, and desktop theming through a simple TOML configuration file.

Built for **Wayland** environments — specifically targeting compositors like [Hyprland](https://hyprland.org/) and [Sway](https://swaywm.org/).

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
git clone https://forgejo.benitez/walkeradmin/temad.git
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

[backends]
# Color extraction backend.
# Supported: "wallust" (default), "matugen", "none"
colors = "wallust"

# Wallpaper setting backend.
# Supported: "swww" (default), "hyprpaper", "wbg"
wallpaper = "swww"

[swww_options]
# These options are passed to `swww img` when the wallpaper backend is "swww".
# See `swww img --help` for all available transition types.
transition_type = "random"
transition_step = 100
transition_fps = 60
transition_duration = 2

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

#### `[backends]`

| Key | Type | Default | Options |
|---|---|---|---|
| `colors` | String | `wallust` | `wallust`, `matugen`, `none` |
| `wallpaper` | String | `swww` | `swww`, `hyprpaper`, `wbg` |

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

### Set a Specific Wallpaper

```bash
temad set ~/Pictures/Wallpapers/sunset.jpg
```

This immediately runs the full pipeline (color extraction → wallpaper → hooks) for the specified image and exits. The path must exist, point to a file, and use a supported image extension (`.jpg`, `.jpeg`, `.png`, `.webp`, `.bmp`, `.gif`).

### Apply the Next Wallpaper

```bash
temad next
```

This selects and applies one wallpaper from the configured directory using `[slideshow].randomize`. In sequential mode it advances from the last successfully applied image; in random mode it avoids immediately repeating the current image when possible. `temad` persists a tiny bounded history in XDG state so `next` and `prev` stay consistent across runs without heavy disk writes.

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
  set          Immediately set a specific wallpaper and extract colors
  next         Apply the next wallpaper from the configured directory
  prev         Apply the previous wallpaper from the configured directory
  completions  Generate shell completion scripts
  help         Print this message or the help of the given subcommand(s)

Options:
  -c, --config <CONFIG>  Path to a custom configuration file
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
├── engine.rs      # Color/wallpaper backends, hooks, CommandRunner trait
└── slideshow.rs   # Image discovery, selection, daemon loop
```

---

## Testing

The project includes **71 unit and integration tests** covering:

- Configuration parsing, defaults, and edge cases
- Path expansion (`~` to home directory)
- Image discovery and filtering
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
