# Agent Guide

- This is one Rust 2018 Cargo crate named `thumbs`.
- The crate builds two binaries: `thumbs` from `src/main.rs` and `tmux-thumbs` from `src/swapper.rs`.
- `thumbs` is the interactive hint picker. It reads pane text from stdin, finds matches, renders hints, and writes the chosen value to stdout or `--target`.
- `tmux-thumbs` is the tmux orchestrator. It shells out to `tmux`, captures pane content, starts `target/release/thumbs`, swaps panes, reads a per-run result file under the system temp directory, and runs the configured pick command.
- Runtime plugin entry is `tmux-thumbs.tmux` -> `tmux-thumbs.sh` -> `target/release/tmux-thumbs` -> `target/release/thumbs`.
- Prefer small, behavior-preserving changes. Existing tmux behavior, CLI flags, and command templating are user-facing API.

## Commands

- Format check: `cargo fmt --all -- --check`.
- Build debug binaries: `cargo build --verbose`.
- Build runtime release binaries: `cargo build --release`.
- Full test suite: `cargo test --verbose`.
- Focus a `thumbs` test: `cargo test --bin thumbs state::tests::match_urls -- --exact`.
- Focus a `tmux-thumbs` test: `cargo test --bin tmux-thumbs tests::quoted_execution -- --exact`.
- CI order in `.github/workflows/rust.yml`: format, build, test, coverage.
- Coverage command: `cargo tarpaulin -o Lcov --output-dir ./coverage`.
- Local coverage requires `cargo-tarpaulin`; CI installs version `0.18.0`.

## Source Map

| Path | Owns |
| --- | --- |
| `src/main.rs` | `thumbs` CLI args, stdin reading, state/view wiring, output formatting, `--target` writes |
| `src/state.rs` | built-in regex patterns, exclude patterns, custom `--regexp`, match priority, capture extraction, hint assignment |
| `src/view.rs` | termion raw mode, alternate screen rendering, keyboard input, navigation, multi-selection, hint display positions |
| `src/swapper.rs` | `tmux-thumbs` CLI args, tmux pane orchestration, option forwarding, per-run result handoff, command execution, OSC52 |
| `src/alphabets.rs` | named alphabets and hint expansion |
| `src/colors.rs` | named colors and `#RRGGBB` parsing |
| `tmux-thumbs.tmux` | `thumbs-pick` command alias and `@thumbs-key` binding |
| `tmux-thumbs.sh` | release-binary/version check, installer launch, top-level option forwarding to `tmux-thumbs` |
| `tmux-thumbs-install.sh` | interactive install/update flow for compile or release download |
| `.github/workflows/rust.yml` | CI checks and non-release artifact builds |
| `.github/workflows/audit.yml` | cargo audit check when dependency manifests change |
| `.github/workflows/release.yml` | release packaging for Linux musl and Apple Darwin |

## Source Of Truth

- Current version is `Cargo.toml` package version.
- Current `thumbs` CLI surface is `app_args()` in `src/main.rs`.
- Current `tmux-thumbs` CLI surface is `app_args()` in `src/swapper.rs`.
- Current tmux user options are split between `tmux-thumbs.sh`, `tmux-thumbs.tmux`, and `src/swapper.rs`.
- README is user-facing documentation, but it can lag implementation. Verify version strings, help output, options, and defaults against source before editing behavior.
- Tests are inline under `#[cfg(test)]` in the source files, not in a separate `tests/` directory.

## Runtime Flow

1. `tmux-thumbs.tmux` registers `thumbs-pick=run-shell -b .../tmux-thumbs.sh` and binds `@thumbs-key` or `space`.
2. `tmux-thumbs.sh` checks `target/release/thumbs` against `Cargo.toml` version, then invokes `target/release/tmux-thumbs`.
3. If `thumbs` is missing or stale, `tmux-thumbs.sh` opens the interactive `tmux-thumbs-install.sh`; avoid this path in automated verification.
4. `tmux-thumbs.sh` passes only `--dir`, `--command`, `--upcase-command`, `--multi-command`, and `--osc52` into `tmux-thumbs`.
5. `src/swapper.rs` reads `tmux show -g`, translates UI options such as `@thumbs-alphabet`, colors, `@thumbs-reverse`, `@thumbs-unique`, `@thumbs-contrast`, and `@thumbs-regexp-*`, then forwards them to `thumbs`.
6. `src/swapper.rs` captures the active pane before pane swapping, starts `thumbs` in a hidden window, synchronizes with `tmux wait-for`, swaps panes, waits for selection, reads and deletes that run's temp result file, and runs the configured command.
7. `src/main.rs` reads stdin, builds `State`, presents `View`, formats selections with `%U` and `%H`, and prints or writes the result.

## Behavior Invariants

- `tmux-thumbs` is not the UI. Keep rendering and interactive hint selection in `thumbs`/`src/view.rs`.
- The wrapper expects release binaries at `target/release/thumbs` and `target/release/tmux-thumbs`; debug builds are not enough for plugin runtime checks.
- Built-in match priority in `src/state.rs` is exclude patterns first, user `--regexp` patterns second, built-ins last.
- Match scanning chooses the earliest next match in a line; ties follow the order produced by the priority list.
- Regex extraction uses named capture `match` when present, otherwise all capture groups, otherwise the full match.
- Bash color escape sequences are consumed by the exclude pattern so they do not become hints and do not break neighboring matches.
- Keep command execution in `src/swapper.rs` safer than literal interpolation: `{}` is intentionally replaced with `${THUMB}` and run through `bash -c 'THUMB="$1"; eval "$2"'` to preserve old config syntax while reducing injection risk.
- Do not remove the tmux capture/start wait ordering in `src/swapper.rs`; it prevents pane resize/reflow from truncating matches before `thumbs` starts.
- Be cautious with the OSC52 sleep in `src/swapper.rs`; it works around tmux redraw timing after the alternate screen exits.
- `src/view.rs` uses termion raw mode and the alternate screen. Unit tests cover helper behavior only; rendering and pane-flow changes need manual tmux smoke testing when feasible.

## Verification Guide

- For Rust code changes, run `cargo fmt --all -- --check` and `cargo test --verbose` unless the task scope clearly justifies a narrower check.
- For regex, match priority, captures, or hint assignment, add or update focused tests in `src/state.rs` and run a focused `cargo test --bin thumbs ... -- --exact` before the full suite.
- For command quoting, command templates, OSC52, tmux option forwarding, or pane orchestration, add or update tests in `src/swapper.rs` and run a focused `cargo test --bin tmux-thumbs ... -- --exact`.
- For alphabets or colors, extend the inline tests in `src/alphabets.rs` or `src/colors.rs`.
- For UI rendering, keyboard handling, alternate-screen behavior, or tmux pane flow, supplement unit tests with a manual smoke test in tmux after `cargo build --release`.
- For shell wrapper or installer changes, build release binaries first if invoking `tmux-thumbs.sh`; otherwise it may open the interactive installer.
- For docs-only changes, tests are usually unnecessary; still check commands and file references against source.

## Manual Smoke Test

- Build runtime binaries with `cargo build --release`.
- In a tmux session, source the plugin with `tmux source-file tmux-thumbs.tmux`.
- Run `thumbs-pick` or press the configured key in a pane containing URLs, paths, SHAs, and numbers.
- Verify normal pick, uppercase pick behavior, multi-select, custom `@thumbs-regexp-*`, and configured command execution if the change touches those areas.

## Editing Rules For This Repo

- Read the relevant source file and its inline tests before editing.
- Keep public CLI flags, tmux option names, defaults, and command-template semantics stable unless the user explicitly asks for a breaking change.
- Prefer adding focused regression tests beside the code you change.
- Use `cargo fmt`; do not hand-format Rust after editing.
- Do not edit generated build output under `target/`.
- Do not invoke `tmux-thumbs.sh` in automation unless release binaries exist and match the package version.
- If you find unrelated worktree changes, leave them alone unless they directly block the task.
