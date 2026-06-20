# Architecture

`tmux-thumbs` is one Cargo crate with two binaries:

- `thumbs`, built from `src/main.rs`, is the interactive hint picker.
- `tmux-thumbs`, built from `src/swapper.rs`, is the tmux orchestrator.

## Runtime Flow

The tmux plugin starts at `tmux-thumbs.tmux`, which registers the `thumbs-pick`
command alias and binds `@thumbs-key` or `space`. That alias runs
`tmux-thumbs.sh`.

`tmux-thumbs.sh` checks that `target/release/thumbs` and
`target/release/tmux-thumbs` exist and report the package version from
`Cargo.toml`. If either release binary is missing or stale, the wrapper opens
`tmux-thumbs-install.sh` in a tmux pane to install or update them.

After validation, `tmux-thumbs.sh` runs `target/release/tmux-thumbs` with the
top-level tmux options it owns, such as `--dir`, `--command`,
`--upcase-command`, `--multi-command`, and `--osc52`.

`src/tmux.rs` handles tmux orchestration. It reads tmux options, translates
picker UI settings into `thumbs` arguments, captures the active pane, starts
`target/release/thumbs` in a hidden tmux window, swaps panes for interaction,
reads the per-run result file from the system temp directory, then runs the
configured command for the selected text.

## Module Responsibilities

- `src/picker.rs` owns the standalone `thumbs` CLI, stdin reading, state/view
  wiring, output formatting, and `--target` writes.
- `src/state.rs` owns built-in regex patterns, user `--regexp` patterns,
  capture extraction, match priority, and hint assignment.
- `src/view.rs` owns terminal raw mode, alternate-screen rendering, keyboard
  input, navigation, multi-selection, and hint display positions.
- `src/tmux.rs` owns tmux command execution, option forwarding, pane swapping,
  per-run result handoff, command execution, and OSC52 output.
- `src/alphabets.rs` and `src/colors.rs` own validation and conversion for
  named alphabets and colors.

Keep the picker UI behavior in `thumbs`/`src/view.rs`. Keep tmux pane flow,
option forwarding, and command execution in `tmux-thumbs`/`src/tmux.rs`.
