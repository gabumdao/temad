# temad Roadmap

## Completed

- [x] Release build and full test suite stabilized locally.
- [x] `.gitignore` updated for GitNexus artifacts, coverage files, and logs.
- [x] `Config` and `HooksConfig` default handling cleaned up.
- [x] `run_daemon_cycle(...)` extracted for clearer daemon flow.
- [x] `swww_options.transition_duration` added.
- [x] `temad set <path>` now validates existence, file type, and supported extension.
- [x] `temad completions <shell>` added with clap-generated completions.
- [x] Local Hyprland integration wired to the installed `temad` binary.
- [x] Hooks trimmed to avoid unnecessary `rustbar` restarts.
- [x] `temad next` added with persisted state tracking.
- [x] `temad prev` upgraded to use bounded real history in random mode.
- [x] Rust CI workflow added for `fmt`, `clippy`, and `test`.
- [x] `temad status` added for non-mutating runtime/config inspection.
- [x] `temad doctor` added for config, dependency, wallpaper directory, and daemon diagnostics.
- [x] Persisted history writes made lock-safe and atomic.
- [x] `temad history` added for read-only inspection of persisted wallpaper history.
- [x] `temad init` added for first-run config bootstrap.
- [x] External command execution now respects configurable timeouts.
- [x] Wallpaper history depth is now configurable.
- [x] Recursive wallpaper scanning added.
- [x] `--dry-run` preview mode added for mutating commands.
- [x] `src/slideshow.rs` split into smaller internal modules.
- [x] Better multi-monitor backend targeting added.
- [x] `temad reload` added to reload daemon config and persisted state without restarting the session.
- [x] Added a remaining-time indicator for the next scheduled wallpaper change.

## In Progress

- [ ] Keep the roadmap synchronized with shipped features and validation status.

## Planned Next

- [ ] Add `temad pause` and `temad play` to pause and resume slideshow progression.
- [ ] Add `temad interval` to update the active wallpaper interval through a command.
- [ ] Reset the slideshow countdown when wallpaper is changed manually via `next` or `prev`.
- [ ] Add lightweight timestamp metadata to wallpaper history entries.
- [ ] Harden `swww-daemon` recovery around stale sockets and startup failures.
- [ ] Polish CLI help text and user-facing command wording, including `--dry-run` help output.

## Future High-Effort Features

- [ ] Integrate with Wallhaven using guest and authenticated API modes, configured through `config.toml`, with local history support.
- [ ] Add Wallhaven tag-driven wallpaper selection flows.
- [ ] Build a network wallpaper provider service (Docker-friendly HTTP API), inspired by Wallhaven-style catalog and delivery workflows.

## Notes

- `temad status` is intentionally human-readable in phase 1.
- CI currently targets the existing single-crate Rust CLI workflow.
- The README should remain the user-facing usage guide; this file is the execution roadmap.
- After replacing the installed binary, Hyprland `exec-once` users must restart `temad daemon` once so the live process matches the new executable and binds the current control socket.
- Quick recovery checks after upgrades: `pgrep -af temad`, `readlink -f /proc/$(pgrep -xo temad)/exe`, `ls /run/user/$(id -u)/temad`, then restart the daemon if the process still points to a deleted binary or no socket is present.
