# Agent Guide

- This is one Rust 2018 Cargo crate named `thumbs`.
- The crate builds two binaries: `thumbs` from `src/main.rs` and `tmux-thumbs` from `src/swapper.rs`; both are thin wrappers around library modules.
- `thumbs` is the interactive hint picker. It reads pane text from stdin, finds matches, renders hints, and writes the chosen value to stdout or `--target`.
- `tmux-thumbs` is the tmux orchestrator. It shells out to `tmux`, captures pane content, starts `target/release/thumbs`, swaps panes, reads a per-run result file under the system temp directory, and runs the configured pick command.
- Runtime plugin entry is `tmux-thumbs.tmux` -> `tmux-thumbs.sh` -> `target/release/tmux-thumbs` -> `target/release/thumbs`.
- Prefer small, behavior-preserving changes. Existing tmux behavior, CLI flags, and command templating are user-facing API.


Before committing or handing work off for review, run the fast full-feature
gate:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --all-features
```

## Source Map

| Path | Owns |
| --- | --- |
| `src/lib.rs` | Library module exports for picker, tmux orchestration, state, view, colors, and alphabets |
| `src/main.rs` | Thin `thumbs` binary wrapper |
| `src/picker.rs` | `thumbs` CLI args, stdin reading, state/view wiring, output formatting, `--target` writes |
| `src/state.rs` | built-in regex patterns, exclude patterns, custom `--regexp`, match priority, capture extraction, hint assignment |
| `src/view.rs` | termion raw mode, alternate screen rendering, keyboard input, navigation, multi-selection, hint display positions |
| `src/swapper.rs` | Thin `tmux-thumbs` binary wrapper |
| `src/tmux.rs` | `tmux-thumbs` CLI args, tmux pane orchestration, option forwarding, per-run result handoff, command execution, OSC52 |
| `src/alphabets.rs` | named alphabets and hint expansion |
| `src/colors.rs` | named colors and `#RRGGBB` parsing |
| `tmux-thumbs.tmux` | `thumbs-pick` command alias and `@thumbs-key` binding |
| `tmux-thumbs.sh` | release-binary/version check, installer launch, top-level option forwarding to `tmux-thumbs` |
| `tmux-thumbs-install.sh` | interactive install/update flow for compile or release download |

## Source Of Truth

- Current version is `Cargo.toml` package version.
- Current `thumbs` CLI surface is `app_args()` in `src/picker.rs`.
- Current `tmux-thumbs` CLI surface is `app_args()` in `src/tmux.rs`.
- Current tmux user options are split between `tmux-thumbs.sh`, `tmux-thumbs.tmux`, and `src/tmux.rs`.
- README is user-facing documentation, but it can lag implementation. Verify version strings, help output, options, and defaults against source before editing behavior.

## Runtime Flow

1. `tmux-thumbs.tmux` registers `thumbs-pick=run-shell -b .../tmux-thumbs.sh` and binds `@thumbs-key` or `space`.
2. `tmux-thumbs.sh` checks `target/release/thumbs` and `target/release/tmux-thumbs` against `Cargo.toml` version, then invokes `target/release/tmux-thumbs`.
3. If either release binary is missing or stale, `tmux-thumbs.sh` opens the interactive `tmux-thumbs-install.sh`; avoid this path in automated verification.
4. `tmux-thumbs.sh` passes only `--dir`, `--command`, `--upcase-command`, `--multi-command`, and `--osc52` into `tmux-thumbs`.
5. `src/tmux.rs` reads `tmux show -g`, translates UI options such as `@thumbs-alphabet`, colors, `@thumbs-reverse`, `@thumbs-unique`, `@thumbs-contrast`, and `@thumbs-regexp-*`, then forwards them to `thumbs`.
6. `src/tmux.rs` captures the active pane before pane swapping, starts `thumbs` in a hidden window, synchronizes with `tmux wait-for`, swaps panes, waits for selection, reads and deletes that run's temp result file, and runs the configured command.
7. `src/picker.rs` reads stdin, builds `State`, presents `View`, formats selections with `%U` and `%H`, and prints or writes the result.

## Behavior Invariants

- `tmux-thumbs` is not the UI. Keep rendering and interactive hint selection in `thumbs`/`src/view.rs`.
- The wrapper expects release binaries at `target/release/thumbs` and `target/release/tmux-thumbs`; debug builds are not enough for plugin runtime checks.
- Built-in match priority in `src/state.rs` is exclude patterns first, user `--regexp` patterns second, built-ins last.
- Match scanning chooses the earliest next match in a line; ties follow the order produced by the priority list.
- Regex extraction uses named capture `match` when present, otherwise all capture groups, otherwise the full match.
- Bash color escape sequences are consumed by the exclude pattern so they do not become hints and do not break neighboring matches.
- Keep command execution in `src/tmux.rs` safer than literal interpolation: `{}` is intentionally replaced with `${THUMB}` and run through `bash -c 'THUMB="$1"; eval "$2"'` to preserve old config syntax while reducing injection risk.
- Do not remove the tmux capture/start wait ordering in `src/tmux.rs`; it prevents pane resize/reflow from truncating matches before `thumbs` starts.
- Be cautious with the OSC52 sleep in `src/tmux.rs`; it works around tmux redraw timing after the alternate screen exits.
- `src/view.rs` uses termion raw mode and the alternate screen. Unit tests cover rendering helpers and captured render output; keyboard handling and pane-flow changes still need manual tmux smoke testing when feasible.

## Verification Guide

- For Rust code changes, run `cargo fmt --all -- --check` and `cargo test --verbose` unless the task scope clearly justifies a narrower check.
- For regex, match priority, captures, or hint assignment, add or update focused tests in `src/state.rs` and run a focused `cargo test --lib state::tests::<name> -- --exact` before the full suite.
- For command quoting, command templates, OSC52, tmux option forwarding, or pane orchestration, add or update tests in `src/tmux.rs` and run a focused `cargo test --lib tmux::tests::<name> -- --exact`.
- For alphabets or colors, extend the inline tests in `src/alphabets.rs` or `src/colors.rs`.
- For UI rendering, keyboard handling, alternate-screen behavior, or tmux pane flow, supplement unit tests with a manual smoke test in tmux after `cargo build --release`.
- For shell wrapper or installer changes, build release binaries first if invoking `tmux-thumbs.sh`; otherwise it may open the interactive installer.
- For docs-only changes, tests are usually unnecessary; still check commands and file references against source.

## Editing Rules For This Repo

- Read the relevant source file and its inline tests before editing.
- Keep public CLI flags, tmux option names, defaults, and command-template semantics stable unless the user explicitly asks for a breaking change.
- Prefer adding focused regression tests beside the code you change.
- Use `cargo fmt`; do not hand-format Rust after editing.
- Do not edit generated build output under `target/`.
- Do not invoke `tmux-thumbs.sh` in automation unless release binaries exist and match the package version.
- If you find unrelated worktree changes, leave them alone unless they directly block the task.
