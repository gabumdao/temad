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

## In Progress

- [ ] Keep the roadmap synchronized with shipped features and validation status.

## Planned Next

- [ ] Add file locking plus atomic writes for persisted history state.
- [ ] Harden `swww-daemon` recovery around stale sockets and startup failures.
- [ ] Make history inspection user-facing via `temad history` or equivalent.
- [ ] Add `temad init` for first-run config and service bootstrap.
- [ ] Support optional recursive wallpaper discovery.
- [ ] Support configurable history depth instead of a fixed internal limit.
- [ ] Add optional command timeouts for external tools and hooks.
- [ ] Consider better multi-monitor targeting for wallpaper backends.
- [ ] Split large modules like `slideshow.rs` as the feature surface grows.

## Notes

- `temad status` is intentionally human-readable in phase 1.
- CI currently targets the existing single-crate Rust CLI workflow.
- The README should remain the user-facing usage guide; this file is the execution roadmap.
