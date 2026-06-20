# tmux-thumbs Architecture Improvement Plan

This plan is for incremental implementation. Do not land it as one large refactor. Each task should preserve the public CLI flags, tmux option names, command template semantics, and the existing runtime entry path unless the task explicitly says otherwise.

Status legend: `[ ]` not started, `[x]` complete, `[!]` blocked or needs a decision.

## Current Top-Level Design

`tmux-thumbs` is one Rust 2018 Cargo crate named `thumbs`. It builds two binaries:

- `thumbs` from `src/main.rs`: standalone interactive picker.
- `tmux-thumbs` from `src/swapper.rs`: tmux orchestrator.

Runtime plugin flow:

```text
tmux-thumbs.tmux
    -> tmux-thumbs.sh
        -> target/release/tmux-thumbs
            -> target/release/thumbs
```

Selection flow:

```text
tmux key binding
    -> shell wrapper validates release picker binary
    -> tmux-thumbs captures active pane text
    -> tmux-thumbs starts hidden thumbs pane
    -> thumbs reads pane text from stdin
    -> State finds matches and assigns hints
    -> View renders hints and collects keyboard input
    -> thumbs writes selection result
    -> tmux-thumbs reads result and runs configured command
```

Important current files:

| File | Current responsibility |
| --- | --- |
| `tmux-thumbs.tmux` | Registers `thumbs-pick` and binds `@thumbs-key`. |
| `tmux-thumbs.sh` | Checks release binary state and forwards only top-level options into `tmux-thumbs`. |
| `src/swapper.rs` | Reads tmux state/options, constructs pane shell command, coordinates pane swap, reads selection, runs command. |
| `src/main.rs` | Parses `thumbs` CLI, reads stdin, builds state/view, writes selected output or `--target`. |
| `src/state.rs` | Built-in regexes, exclude regexes, custom regexes, match priority, capture extraction, hint assignment. |
| `src/view.rs` | Terminal raw mode, alternate screen rendering, keyboard loop, multi-selection, hint positions. |
| `src/alphabets.rs` | Hint alphabet lookup and expansion. |
| `src/colors.rs` | Named and RGB color parsing. |

## Behavior Invariants To Preserve

- Keep `thumbs` usable standalone; do not make it depend on tmux.
- Keep `tmux-thumbs` as orchestration only; do not move rendering into it.
- Keep the runtime expectation that release binaries live under `target/release/` for plugin use.
- Keep existing CLI flags and tmux option names stable unless a user explicitly approves a breaking change.
- Keep command template compatibility: configured commands may contain `{}`, and `{}` currently means the selected text.
- Keep the safer command execution idea: selected text must not be literally spliced into shell commands.
- Keep match priority in `src/state.rs`: exclude patterns first, custom `--regexp` second, built-ins last.
- Keep capture-before-pane-swap ordering; it prevents tmux pane resize/reflow from truncating matches.
- Keep cancellation behavior user-friendly: pressing escape or making no selection should not run the configured command.

## Known High-Risk Areas

| Area | Current issue | Reference |
| --- | --- | --- |
| Result handoff | Fixed global `/tmp/thumbs-last` can collide across sessions and stale runs. | `src/swapper.rs:35`, `src/swapper.rs:318-330` |
| Orchestration state | `Swapper` methods rely on hidden `Option` fields set by previous calls. | `src/swapper.rs:48-64`, `src/swapper.rs:496-504` |
| Shell generation | Large shell strings mix tmux commands, paths, options, and user config. | `src/swapper.rs:155-189`, `src/swapper.rs:216-224` |
| Error handling | `Executor` returns stdout only and ignores status/stderr. | `src/swapper.rs:10-32` |
| Wrapper validation | Wrapper checks `thumbs` but not `tmux-thumbs`; final execution is hidden by `|| true`. | `tmux-thumbs.sh:8-18`, `tmux-thumbs.sh:53` |
| UI edge cases | Reverse mode underflows on empty matches; rendering writes to real stdout despite a writer parameter. | `src/view.rs:56-58`, `src/view.rs:99-183` |
| Stringly typed config | Hint positions, colors, tmux booleans, and command options are parsed as strings in multiple places. | `src/main.rs:26-142`, `src/swapper.rs:436-471` |

## Recommended Implementation Order

Implement Part 1 first. It removes race conditions and fundamental architecture hazards. Implement Part 2 after Part 1 stabilizes, because type and module cleanup is safer once the workflow boundaries are explicit.

Do not combine unrelated tasks. A good pull request should complete one task or one small checkpoint.

---

# Part 1: Design Improvements That Eliminate Fundamental Flaws And Race Conditions

## 1.1 Establish Regression Tests For Current Orchestration Contracts

**Goal:** Lock down the intended tmux orchestration behavior before changing design.

**Context:** Existing tests in `src/swapper.rs` cover active pane detection, swap command creation, capture/start ordering, and command quoting. More tests are needed around cancellation, result file handling, binary checks, option forwarding, and tmux command failures.

**Steps:**

- [x] Add focused tests in `src/swapper.rs` for the current no-selection path.
- [x] Add a test that malformed or empty selection content does not run a configured command.
- [x] Add a test that the generated picker command writes to the same result path that retrieval reads.
- [x] Add a test for tmux option forwarding with values containing spaces.
- [x] Add a test for boolean option parsing behavior before changing it.

**Acceptance criteria:**

- [x] Tests document the intended behavior without requiring real tmux.
- [x] The tests fail if result retrieval and picker target paths diverge.
- [x] The tests fail if cancellation triggers command execution.

**Verification:**

- [x] Run `cargo test --bin tmux-thumbs tests::quoted_execution -- --exact`.
- [x] Run `cargo test --bin tmux-thumbs --verbose`.

**Dependencies:** None.

**Likely files:** `src/swapper.rs`.

## 1.2 Replace The Global Result File With A Per-Run Result Path

**Goal:** Eliminate collisions and stale reads caused by the fixed `/tmp/thumbs-last` file.

**Context:** `src/swapper.rs` currently uses `const TMP_FILE: &str = "/tmp/thumbs-last"`. Every tmux session and every invocation shares that path. The `Swapper` already creates a unique signal id from timestamp data; the result path can use the same uniqueness.

**Steps:**

- [x] Replace `TMP_FILE` with a per-run `result_path: String` field on the orchestration context.
- [x] Generate the result path from `std::env::temp_dir()` and the existing unique signal id, for example `thumbs-last-<signal_id>`.
- [x] Pass the per-run path into the generated `thumbs -t <path>` command.
- [x] Read and remove the same per-run path in retrieval/cleanup code.
- [x] Treat a missing result file as cancellation, not as an orchestration error.
- [x] Ensure cleanup attempts to remove only the per-run file.

**Acceptance criteria:**

- [x] Two `Swapper` instances generate different result paths.
- [x] The picker target and retrieval path are identical in tests.
- [x] A missing result file produces no configured command execution.
- [x] No code path still depends on `/tmp/thumbs-last`.

**Verification:**

- [x] Run `cargo test --bin tmux-thumbs --verbose`.
- [x] Run `cargo test --verbose`.

**Dependencies:** Task 1.1.

**Likely files:** `src/swapper.rs`, `AGENTS.md` after implementation to update stale references.

## 1.3 Make Command Execution Fallible And Observable

**Goal:** Stop losing real tmux, shell, and filesystem failures.

**Context:** `Executor::execute()` returns only trimmed stdout. `RealShell` ignores exit status and stderr. This hides errors from `tmux`, `cat`, `rm`, `bash`, and the generated pane command.

**Steps:**

- [x] Introduce a `CommandOutput` struct with `stdout`, `stderr`, and `status` or `success`.
- [x] Change `Executor::execute` to return `Result<CommandOutput, Error>`.
- [x] Update `RealShell` to capture nonzero exits with command, status, and stderr.
- [x] Update `TestShell` to provide status-aware fake outputs.
- [x] Explicitly model cancellation separately from errors.
- [x] Ensure `tmux-thumbs` exits `0` for cancellation and nonzero for real orchestration errors.
- [x] Preserve command context in errors so Task 1.4 can add higher-level phase context without losing low-level detail.

**Acceptance criteria:**

- [x] A failing tmux command can be observed in tests.
- [x] Cancellation does not look like a shell error.
- [x] Main returns a clear error for actual orchestration failure.
- [x] Errors include the command, exit status, and stderr when those are available.
- [x] Existing successful command sequencing still passes.

**Verification:**

- [x] Run `cargo test --bin tmux-thumbs --verbose`.
- [x] Run `cargo test --verbose`.

**Dependencies:** Task 1.2.

**Likely files:** `src/swapper.rs`.

## 1.4 Add Structured Diagnostics For Orchestration Failures

**Goal:** Make failures traceable enough that a user or agent can tell which phase failed, which command failed, and which run-specific identifiers were involved.

**Context:** `tmux-thumbs` coordinates multiple tmux commands, a generated shell script, wait-for signals, a result file, and a final user command. Without structured context, failures look like generic panics, silent no-ops, or hangs. Task 1.3 gives command-level status/stderr; this task adds orchestration-level context on top.

**Trace context to capture:**

```text
run id
phase name
active pane id when known
thumbs pane id when known
result path
wait-for signal names
command args, status, stdout, stderr when a command fails
```

**Steps:**

- [ ] Add an orchestration error type that can wrap command errors with a phase label.
- [ ] Define phase labels such as `capture_active_pane`, `start_picker`, `wait_capture`, `swap_panes`, `resize_pane`, `start_thumbs`, `wait_thumbs`, `read_selection`, `cleanup_result`, and `execute_command`.
- [ ] Add a lightweight run context struct that carries the run id, wait-for signal names, result path, and pane ids as they become known.
- [ ] Add a debug output path controlled by an opt-in mechanism, for example `THUMBS_DEBUG=1`, without changing normal tmux UX.
- [ ] Ensure normal cancellation stays quiet and does not print diagnostics unless debug mode is enabled.
- [ ] Ensure real failures produce enough information on stderr or tmux display-message to identify the failing phase.
- [ ] Add tests that a simulated command failure reports both phase context and command context.
- [ ] Add tests that cancellation is not reported as a failure in normal mode.

**Acceptance criteria:**

- [ ] A failing command can be traced to both a low-level command and a high-level orchestration phase.
- [ ] Debug mode includes run id, result path, and wait-for signal names.
- [ ] Normal successful picks and cancellations do not produce noisy diagnostics.
- [ ] Diagnostics never include selected text unless it is already part of the failing user command context and debug mode is enabled.

**Verification:**

- [ ] Run `cargo test --bin tmux-thumbs --verbose`.
- [ ] Manually run with `THUMBS_DEBUG=1` in tmux after `cargo build --release` for one success and one cancellation.

**Dependencies:** Task 1.3.

**Likely files:** `src/swapper.rs`, possibly `tmux-thumbs.sh` if debug env forwarding needs wrapper changes.

## 1.5 Refactor `Swapper` Into An Explicit Workflow

**Goal:** Remove temporal coupling where public methods must be called in a precise order to populate internal `Option` fields.

**Context:** `Swapper` currently stores `active_pane_id`, `active_pane_height`, `active_pane_scroll_position`, `active_pane_zoomed`, `thumbs_pane_id`, and `content` as mutable `Option` fields. Later methods call `unwrap()` on those fields. The only thing enforcing order is `main()`.

**Target shape:**

```text
Swapper::run() -> Result<RunOutcome>
    capture_active_pane() -> Result<ActivePane>
    start_picker(&ActivePane) -> Result<ThumbsPane>
    wait_for_capture()
    swap_panes(&ActivePane, &ThumbsPane)
    resize_if_needed(&ActivePane, &ThumbsPane)
    start_thumbs()
    wait_thumbs()
    read_selection() -> Result<Option<Vec<Selection>>>
    execute_selection(Vec<Selection>)
```

**Steps:**

- [ ] Create an `ActivePane` struct with pane id, height, optional scroll position, and zoom state.
- [ ] Create a `ThumbsPane` struct with pane id.
- [ ] Create a `RunSignals` struct for finished, captured, and start wait-for names.
- [ ] Make step functions private unless tests need them directly.
- [ ] Make `main()` call only `swapper.run()`.
- [ ] Remove internal `Option` fields that only exist to pass data between steps.

**Acceptance criteria:**

- [ ] It is impossible to call `swap_panes` without explicit `ActivePane` and `ThumbsPane` data.
- [ ] `main()` no longer contains the orchestration sequence line-by-line.
- [ ] Tests still verify the same command ordering.
- [ ] `unwrap()` on orchestration state is removed from production paths.
- [ ] Each workflow step attaches the phase label defined in Task 1.4 to errors.

**Verification:**

- [ ] Run `cargo test --bin tmux-thumbs --verbose`.
- [ ] Run `cargo test --verbose`.

**Dependencies:** Task 1.4.

**Likely files:** `src/swapper.rs`.

## 1.6 Add Robust Tmux Pane Parsing

**Goal:** Make tmux output parsing explicit and testable.

**Context:** Active pane parsing currently splits output lines by `:` and indexes fields with `unwrap()`. It assumes every line has six fields and uses `active` as the final marker.

**Steps:**

- [ ] Change the tmux format string to use a delimiter that cannot appear in the formatted numeric fields, such as tab.
- [ ] Add `ActivePane::parse_list_panes(output: &str) -> Result<ActivePane>`.
- [ ] Return clear errors for missing active pane, malformed lines, invalid height, invalid scroll position, and invalid zoom flag.
- [ ] Add table-style tests for valid active pane, no active pane, malformed line, and copy-mode scroll state.

**Acceptance criteria:**

- [ ] No production parser uses unchecked field indexing.
- [ ] Malformed tmux output is a controlled error.
- [ ] Existing active pane behavior is preserved.

**Verification:**

- [ ] Run `cargo test --bin tmux-thumbs tests::retrieve_active_pane -- --exact`.
- [ ] Run `cargo test --bin tmux-thumbs --verbose`.

**Dependencies:** Task 1.5.

**Likely files:** `src/swapper.rs`.

## 1.7 Centralize Shell Quoting And Generated Pane Script Construction

**Goal:** Reduce command breakage and injection risk in the generated tmux pane shell script.

**Context:** `execute_thumbs()` builds a single large shell command with interpolated `dir`, temp path, tmux pane ids, flags, regex values, color values, and wait-for signal names. Values are manually wrapped in single quotes in some places, but not escaped centrally.

**Steps:**

- [ ] Introduce a small shell quoting helper for values inserted into shell script text.
- [ ] Use the helper for `dir`, picker binary path, result path, option values, regexp values, pane ids, and signal names.
- [ ] Build the pane script from named pieces, not one large `format!` call.
- [ ] Keep the current capture-before-start ordering.
- [ ] Add tests for option values containing spaces, single quotes, double quotes, and backslashes.
- [ ] Add tests that generated script still contains capture, capture signal, start wait, picker invocation, pane restore, and finished signal in the required order.

**Acceptance criteria:**

- [ ] No untrusted tmux option value is inserted into the shell script unquoted.
- [ ] Existing custom regexp behavior is preserved.
- [ ] Tests document shell escaping for troublesome values.

**Verification:**

- [ ] Run `cargo test --bin tmux-thumbs --verbose`.
- [ ] Run `cargo test --verbose`.

**Dependencies:** Task 1.6.

**Likely files:** `src/swapper.rs`.

## 1.8 Make Tmux Wait Signaling Failure-Safe

**Goal:** Prevent hangs when the picker pane exits early or the generated shell script fails before normal completion.

**Context:** The orchestrator waits on tmux signals. The generated pane script sends the finished signal at the end. If the script fails before reaching that command because of a shell syntax issue or unexpected failure, the waiting orchestrator can hang.

**Steps:**

- [ ] Add an `EXIT` trap in the generated pane script to always signal the finished wait-for channel.
- [ ] Ensure the trap does not mask the separate capture/start synchronization ordering.
- [ ] Make result-file absence after finished signal mean cancellation or picker failure, depending on exit status if available.
- [ ] Add tests that the generated script contains the trap before risky commands.
- [ ] Add tests that the finished signal still happens after normal picker completion.

**Acceptance criteria:**

- [ ] A generated script failure cannot leave `tmux-thumbs` waiting forever on the finished signal.
- [ ] Existing split-pane capture ordering is preserved.
- [ ] Cancellation remains non-disruptive.

**Verification:**

- [ ] Run `cargo test --bin tmux-thumbs tests::waits_for_capture_before_swapping_split_panes -- --exact`.
- [ ] Run `cargo test --bin tmux-thumbs --verbose`.

**Dependencies:** Task 1.7.

**Likely files:** `src/swapper.rs`.

## 1.9 Parse Tmux Options Into A Typed Picker Argument Model

**Goal:** Remove ad hoc option translation and make boolean/string/regexp behavior explicit.

**Context:** `src/swapper.rs` reads `tmux show -g` and parses lines with a regex. Boolean options `reverse`, `unique`, and `contrast` become enabled if present, regardless of value. String and regexp options are pushed directly into a shell argument string.

**Steps:**

- [ ] Create a `ThumbsCliArgs` or `PickerArgs` struct that stores values before shell rendering.
- [ ] Parse `@thumbs-alphabet`, `@thumbs-position`, color options, and `@thumbs-regexp-*` into that struct.
- [ ] Parse booleans with documented accepted true values. Preserve existing documented values such as `enabled` and `1` where applicable.
- [ ] Ignore unset or empty values consistently.
- [ ] Render picker args only after typed parsing and shell quoting.
- [ ] Add tests for `enabled`, `1`, `0`, `disabled`, empty, quoted strings, and regex backslashes.

**Acceptance criteria:**

- [ ] Tmux option parsing is unit-testable without constructing a pane command.
- [ ] Boolean behavior is documented by tests.
- [ ] Rendering picker args is separate from parsing tmux output.

**Verification:**

- [ ] Run `cargo test --bin tmux-thumbs --verbose`.

**Dependencies:** Task 1.7.

**Likely files:** `src/swapper.rs`, optionally a new `src/tmux_options.rs` after `lib.rs` exists.

## 1.10 Validate Both Runtime Binaries In The Wrapper

**Goal:** Prevent silent runtime failure when `target/release/tmux-thumbs` is missing or stale.

**Context:** `tmux-thumbs.sh` checks `target/release/thumbs` against `Cargo.toml` version, but it also invokes `target/release/tmux-thumbs`. The final command is currently followed by `|| true`.

**Steps:**

- [ ] Check that both `thumbs` and `tmux-thumbs` exist before invoking the orchestrator.
- [ ] Check both `--version` outputs against `Cargo.toml` version.
- [ ] Preserve the interactive installer/update behavior for missing or stale binaries.
- [ ] After Task 1.3 gives the orchestrator clean cancellation semantics, remove or narrow `|| true` so real failures are visible.
- [ ] Add shell-level tests if a shell test harness exists; otherwise document manual verification.

**Acceptance criteria:**

- [ ] Missing `tmux-thumbs` triggers the installer path instead of a hidden failed command.
- [ ] Stale `tmux-thumbs` triggers the update path.
- [ ] User cancellation does not produce noisy tmux errors.
- [ ] Real orchestrator failures are not swallowed silently.

**Verification:**

- [ ] Run `cargo build --release` before invoking wrapper manually.
- [ ] In tmux, source `tmux-thumbs.tmux` and run `thumbs-pick`.
- [ ] Temporarily move one release binary aside in a controlled test and verify installer/update path.

**Dependencies:** Task 1.3. If wrapper debug behavior changes, also depend on Task 1.4.

**Likely files:** `tmux-thumbs.sh`, maybe `AGENTS.md` if verification guidance changes.

## 1.11 Checkpoint: Race-Condition, Traceability, And Orchestration Review

**Goal:** Confirm Part 1 removed the architectural hazards without changing user-facing behavior.

**Checklist:**

- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo test --bin tmux-thumbs --verbose` passes.
- [ ] `cargo test --verbose` passes.
- [ ] `cargo build --release` passes.
- [ ] Manual tmux smoke test passes normal pick, uppercase pick, multi-select, custom regexp, and cancellation.
- [ ] Review generated pane script for quoting, wait signaling, and cleanup.
- [ ] Verify one simulated failure reports the failing phase, command context, and run identifiers.
- [ ] Verify normal cancellation stays quiet unless debug mode is enabled.
- [ ] Confirm no code path references fixed `/tmp/thumbs-last`.
- [ ] Confirm no public CLI flags or tmux option names changed.

---

# Part 2: Code Improvements For Idiomatic, Type-Safe, Concise, Maintainable Rust

## 2.1 Introduce `src/lib.rs` And Make Binaries Thin Wrappers

**Goal:** Make shared code reusable and easier to test outside binary entrypoints.

**Context:** `src/main.rs` declares `mod alphabets; mod colors; mod state; mod view;`. `src/swapper.rs` is a separate binary with orchestration code and tests. There is no library crate boundary even though the crate has reusable modules.

**Steps:**

- [ ] Add `src/lib.rs` exporting `alphabets`, `colors`, `state`, and `view`.
- [ ] Move tmux orchestration support into a module such as `src/swapper.rs` or `src/tmux.rs`, then point the binary to a thin `src/bin/tmux-thumbs.rs` or keep the current binary path with a small wrapper.
- [ ] Keep binary names unchanged in `Cargo.toml`.
- [ ] Keep existing inline tests or move them beside the modules they test.
- [ ] Update imports to use the library crate path where appropriate.

**Acceptance criteria:**

- [ ] Both binaries still build with the same names.
- [ ] `main()` functions only parse CLI, call library functions, and map errors to exit codes.
- [ ] Unit tests can target library modules directly.

**Verification:**

- [ ] Run `cargo build --verbose`.
- [ ] Run `cargo test --verbose`.

**Dependencies:** Part 1 recommended, but this can start after Task 1.5 if needed.

**Likely files:** `Cargo.toml`, `src/lib.rs`, `src/main.rs`, `src/swapper.rs`, possibly `src/bin/tmux-thumbs.rs`.

## 2.2 Define Typed Configuration Structs For CLI And Runtime Options

**Goal:** Replace repeated `ArgMatches` lookups and loose string passing with clear data structures.

**Context:** `src/main.rs` repeatedly calls `args.get_one::<String>(...).unwrap()`, then passes primitive strings and booleans into `State` and `View`. `src/swapper.rs` separately parses its own command config.

**Steps:**

- [ ] Create `ThumbsOptions` for standalone picker settings: alphabet, format, colors, multi, reverse, unique, contrast, position, regexps, target.
- [ ] Create `SwapperOptions` for tmux orchestrator settings: plugin dir, command templates, OSC52.
- [ ] Implement `TryFrom<&ArgMatches>` or simple `from_matches` constructors that return `Result`.
- [ ] Keep clap defaults as the source of defaults or centralize defaults in constants used by clap and tests.
- [ ] Update `main()` functions to use the typed config.

**Acceptance criteria:**

- [ ] Repeated raw `get_one(...).unwrap()` calls disappear from production `main()` code.
- [ ] Invalid user-facing values can produce a controlled error.
- [ ] Defaults remain unchanged.

**Verification:**

- [ ] Run `cargo test --verbose`.
- [ ] Run `target/debug/thumbs --help` after build and compare expected flags/defaults.

**Dependencies:** Task 2.1.

**Likely files:** `src/main.rs`, `src/swapper.rs`, new config module.

## 2.3 Replace Stringly Typed Enums With Real Types

**Goal:** Prevent invalid state from spreading through the program.

**Context:** Hint position is a raw `&str`; colors are parsed late and panic on invalid names; alphabet lookup panics on unknown names. These are user-facing inputs and should be parsed at boundaries.

**Steps:**

- [ ] Add a `HintPosition` enum with `Left`, `Right`, `OffLeft`, and `OffRight`.
- [ ] Parse `--position` and `@thumbs-position` into `HintPosition` once.
- [ ] Add a `ColorSpec` or `ParsedColor` wrapper if helpful, while preserving termion rendering behavior.
- [ ] Change alphabet lookup to return `Result<Alphabet, Error>` or validate alphabet names during config parsing.
- [ ] Replace string comparisons in rendering with enum matches.
- [ ] Add tests for invalid position, invalid color, invalid alphabet, and all valid positions.

**Acceptance criteria:**

- [ ] `ViewOptions.position` is not a raw string.
- [ ] User input errors are handled at parse time.
- [ ] Rendering cannot receive an unknown hint position.

**Verification:**

- [ ] Run focused tests in `src/view.rs`, `src/colors.rs`, and `src/alphabets.rs`.
- [ ] Run `cargo test --verbose`.

**Dependencies:** Task 2.2.

**Likely files:** `src/view.rs`, `src/main.rs`, `src/swapper.rs`, `src/colors.rs`, `src/alphabets.rs`.

## 2.4 Use More Appropriate Coordinate And Index Types

**Goal:** Remove avoidable casts, underflows, and signed/unsigned confusion.

**Context:** `state::Match` stores `x` and `y` as `i32`, while indexing and terminal positioning use `usize` and `u16`. `View::new` computes `matches.len() - 1` in reverse mode even when there are zero matches. `off_left` computes a negative offset through unsigned arithmetic.

**Steps:**

- [ ] Change match coordinates to `usize` where they represent indexes into line text.
- [ ] Convert to terminal coordinates at the rendering boundary with checked or saturating conversion.
- [ ] Fix reverse mode empty-match behavior with `checked_sub`, `saturating_sub`, or explicit empty handling.
- [ ] Fix `off_left` using signed arithmetic from the beginning or `saturating_sub`.
- [ ] Add tests for empty input with `--reverse`, `off_left` near column zero, and wide-character prefixes.

**Acceptance criteria:**

- [ ] Empty reverse mode cannot panic.
- [ ] `off_left` cannot underflow.
- [ ] Coordinate conversions are localized and readable.

**Verification:**

- [ ] Run `cargo test --bin thumbs view::tests::hint_text -- --exact` and new focused view tests.
- [ ] Run `cargo test --verbose`.

**Dependencies:** Task 2.3.

**Likely files:** `src/state.rs`, `src/view.rs`.

## 2.5 Precompile Built-In Regexes And Separate Pattern Matching From Hint Assignment

**Goal:** Make matching faster, clearer, and easier to modify safely.

**Context:** `State::matches()` recompiles exclude and built-in regexes on every call, compiles custom regexes inside matching, scans each line, extracts captures, and assigns hints all in one function.

**Steps:**

- [ ] Precompile built-in and exclude regexes once using existing `lazy_static`.
- [ ] Compile custom regexes during state/config construction and return a user-facing error for invalid regexps.
- [ ] Extract helpers for pattern priority, capture extraction, line scanning, and hint assignment.
- [ ] Keep priority unchanged: exclude, custom, built-in.
- [ ] Add regression tests around named capture `match`, multiple capture groups, full-match fallback, and priority ties.

**Acceptance criteria:**

- [ ] Built-in regexes are not compiled on every `matches()` call.
- [ ] Invalid custom regex is handled before interactive UI starts.
- [ ] Existing state tests still pass unchanged or with clearer names.

**Verification:**

- [ ] Run focused state tests, for example `cargo test --bin thumbs state::tests::priority -- --exact`.
- [ ] Run `cargo test --verbose`.

**Dependencies:** Task 2.2.

**Likely files:** `src/state.rs`, `src/main.rs`.

## 2.6 Prefer Slices And Owned Configuration Over Borrowing `Vec`

**Goal:** Make APIs idiomatic and less tied to caller storage.

**Context:** `State::new` takes `&Vec<&str>` and `&Vec<&str>`. This is less idiomatic than slices and makes lifetimes more complicated than necessary.

**Steps:**

- [ ] Change functions that read collections to accept slices such as `&[&str]`.
- [ ] Store custom regex config in a typed owned structure when practical.
- [ ] Avoid exposing `pub lines: &'a Vec<&'a str>`; expose only what `View` needs or use an accessor.
- [ ] Update tests to pass slices directly.

**Acceptance criteria:**

- [ ] No public function requires `&Vec<T>` when `&[T]` is enough.
- [ ] Lifetimes become easier to follow, not more complex.

**Verification:**

- [ ] Run `cargo test --bin thumbs --verbose`.
- [ ] Run `cargo test --verbose`.

**Dependencies:** Task 2.5.

**Likely files:** `src/state.rs`, `src/view.rs`, `src/main.rs`.

## 2.7 Make Selection Parsing And Command Choice Typed

**Goal:** Replace string splitting in command execution with explicit selection data.

**Context:** `thumbs` emits lines formatted as `%U:%H`. `tmux-thumbs` parses them with `splitn(2, ':')`. Multi-selection currently joins parsed values with spaces and does not strongly distinguish malformed lines from cancellation.

**Steps:**

- [ ] Add a `Selection` struct with `text: String` and `upcase: bool`.
- [ ] Add `Selection::parse_line` for `%U:%H` lines.
- [ ] Add `SelectionSet` or helper functions for single vs multi selection.
- [ ] Treat empty content as cancellation.
- [ ] Treat malformed non-empty content as an error.
- [ ] Preserve support for selected text containing `:` by continuing to split only once.

**Acceptance criteria:**

- [ ] Command selection no longer depends on raw tuple/string logic.
- [ ] Malformed result content is detectable in tests.
- [ ] Multi-selection behavior is unchanged for valid selections.

**Verification:**

- [ ] Run `cargo test --bin tmux-thumbs --verbose`.
- [ ] Run `cargo test --verbose`.

**Dependencies:** Task 1.3.

**Likely files:** `src/swapper.rs`, possible new selection module after `lib.rs`.

## 2.8 Improve View Testability By Separating Rendering From Terminal IO

**Goal:** Make UI behavior testable without a real terminal.

**Context:** `View::render(&mut dyn Write, ...)` writes cursor hide and flush to the passed writer, but most visible output uses `print!` against global stdout. `View::present()` hardcodes `async_stdin`, raw mode, alternate screen, and sleep.

**Steps:**

- [ ] Replace `print!` calls inside `render` with `write!(stdout, ...)`.
- [ ] Keep raw mode and alternate screen setup in `present`, not in pure rendering helpers.
- [ ] Extract hint placement calculation into a pure helper.
- [ ] Add tests for `left`, `right`, `off_left`, `off_right`, contrast mode, selected match color choice if practical, and typed hint overlay.
- [ ] Consider an input/event abstraction only after rendering output is testable.

**Acceptance criteria:**

- [ ] Rendering tests can capture output with an in-memory writer.
- [ ] No rendering helper writes to global stdout.
- [ ] Terminal setup remains isolated to the outer presentation layer.

**Verification:**

- [ ] Run `cargo test --bin thumbs view::tests -- --nocapture` if useful.
- [ ] Run `cargo test --verbose`.

**Dependencies:** Task 2.4.

**Likely files:** `src/view.rs`.

## 2.9 Simplify Alphabet And Color Lookup

**Goal:** Reduce allocation and panic-based control flow in small utility modules.

**Context:** `get_alphabet` builds a `HashMap` on every call. `get_color` returns boxed termion colors and panics for invalid input. Both are small but user-facing enough to benefit from clear parse errors.

**Steps:**

- [ ] Replace per-call `HashMap` construction with a simple iterator lookup over `ALPHABETS` or a lazily initialized map.
- [ ] Return `Result<Alphabet, Error>` for unknown alphabet or validate before lookup.
- [ ] Return `Result<Box<dyn color::Color>, Error>` or parse colors into a custom enum that renders later.
- [ ] Keep named colors and `#RRGGBB` behavior unchanged.
- [ ] Update tests currently using `#[should_panic]` to assert errors instead.

**Acceptance criteria:**

- [ ] Unknown alphabet/color does not panic in production flows.
- [ ] Existing valid color/alphabet tests still pass.
- [ ] Utility modules are shorter or clearer than before.

**Verification:**

- [ ] Run `cargo test --bin thumbs alphabets::tests` and `cargo test --bin thumbs colors::tests`.
- [ ] Run `cargo test --verbose`.

**Dependencies:** Task 2.3.

**Likely files:** `src/alphabets.rs`, `src/colors.rs`, `src/main.rs`, `src/view.rs`.

## 2.10 Remove Dead Or Misleading Code And Comments

**Goal:** Reduce noise for future maintainers and agents.

**Context:** There are dead debug helpers, an empty `send_osc52` method, and comments with stale terms such as `rustbox` even though the UI uses termion.

**Steps:**

- [ ] Remove unused `dbg` helpers or put them behind a deliberate debug feature.
- [ ] Remove the empty `send_osc52` method or implement it as part of a real OSC52 abstraction.
- [ ] Update the OSC52 redraw comment to mention current termion alternate-screen behavior.
- [ ] Fix typos in comments and clap help only when touching nearby code.
- [ ] Avoid broad cosmetic churn unrelated to a functional task.

**Acceptance criteria:**

- [ ] No empty public methods remain.
- [ ] Comments describe current code, not old dependencies.
- [ ] Cleanup does not change behavior.

**Verification:**

- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run `cargo test --verbose`.

**Dependencies:** Can be done opportunistically after touched areas stabilize.

**Likely files:** `src/main.rs`, `src/swapper.rs`, `src/state.rs`.

## 2.11 Update Documentation And Agent Context After Refactors

**Goal:** Keep user-facing docs and agent guidance aligned with implementation.

**Context:** README may lag source. `AGENTS.md` records current runtime flow and invariants. After Part 1 and Part 2, references to `/tmp/thumbs-last`, wrapper validation, module layout, and verification steps may become stale.

**Steps:**

- [ ] Update README only for user-visible behavior, options, or help output changes.
- [ ] Update `AGENTS.md` for new module layout, result handoff, error semantics, and verification guidance.
- [ ] If a new library module layout exists, update the source map.
- [ ] If wrapper behavior changes, update install/runtime notes.

**Acceptance criteria:**

- [ ] Docs do not mention removed implementation details as current behavior.
- [ ] Agent instructions remain enough for a new agent to work safely.
- [ ] README and source defaults do not contradict each other for touched options.

**Verification:**

- [ ] Read `AGENTS.md` and touched README sections after edits.
- [ ] For docs-only changes, tests are optional unless code changed in the same task.

**Dependencies:** Complete after the relevant implementation tasks.

**Likely files:** `README.md`, `AGENTS.md`, this plan file if tasks are checked off.

## 2.12 Checkpoint: Maintainability Review

**Goal:** Confirm Part 2 improved code clarity without broad behavior drift.

**Checklist:**

- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo build --verbose` passes.
- [ ] `cargo test --verbose` passes.
- [ ] `cargo build --release` passes if runtime wrapper changed.
- [ ] Focused state tests pass if matching changed.
- [ ] Focused swapper tests pass if orchestration changed.
- [ ] Manual tmux smoke test passes if UI, wrapper, command execution, pane orchestration, or OSC52 changed.
- [ ] Public flags and tmux option names are unchanged.
- [ ] Invalid input produces controlled errors rather than panics where the task touched parsing.

---

# Suggested Parallelization

Safe after Task 1.1:

- One agent can work on result-path isolation in Task 1.2.
- Another agent can work on test scaffolding for robust tmux pane parsing in Task 1.6.

Safe after Task 1.7:

- One agent can work on wrapper validation in Task 1.10.
- Another agent can work on typed selection parsing in Task 2.7.

Do not parallelize without coordination:

- `src/swapper.rs` state-machine refactor and executor error handling, because they touch the same core flow.
- `src/lib.rs` extraction and large module moves, because they create import churn across the whole crate.
- UI rendering refactor and coordinate type changes, because both touch hint placement.

# Global Verification Matrix

Use the narrowest useful checks during implementation, then run the broader checks at each checkpoint.

| Change area | Focused verification | Broader verification |
| --- | --- | --- |
| Regex matching or hint assignment | `cargo test --bin thumbs state::tests::priority -- --exact` | `cargo test --bin thumbs --verbose` |
| Tmux command quoting or orchestration | `cargo test --bin tmux-thumbs tests::quoted_execution -- --exact` | `cargo test --bin tmux-thumbs --verbose` |
| Pane capture/start ordering | `cargo test --bin tmux-thumbs tests::waits_for_capture_before_swapping_split_panes -- --exact` | `cargo test --bin tmux-thumbs --verbose` |
| Colors or alphabets | Focused module tests in `src/colors.rs` or `src/alphabets.rs` | `cargo test --bin thumbs --verbose` |
| UI rendering or input | Focused `view` tests plus manual smoke where feasible | `cargo test --verbose` and tmux smoke test |
| Shell wrapper | `cargo build --release` before invoking wrapper | Manual tmux smoke test |
| Docs only | Read changed docs for stale references | No tests required unless code changed |

# Manual Tmux Smoke Test

Run this after any change touching `src/swapper.rs`, `src/view.rs`, shell wrappers, command execution, or OSC52.

```text
1. Build release binaries with `cargo build --release`.
2. Start or use a tmux session.
3. Source the plugin with `tmux source-file tmux-thumbs.tmux`.
4. Open a pane containing URLs, file paths, SHAs, numbers, and text matching a custom regexp.
5. Run `thumbs-pick` or press the configured key.
6. Verify normal pick copies the selected value.
7. Verify uppercase hint behavior runs the upcase command.
8. Verify multi-select collects multiple values.
9. Verify escape/cancel runs no command and leaves no stale result file.
10. Verify custom `@thumbs-regexp-*` is forwarded and matched.
11. Verify configured command templates still work with `{}`.
```

# Final Definition Of Done

- [ ] Part 1 checkpoint is complete.
- [ ] Part 2 checkpoint is complete.
- [ ] No fixed `/tmp/thumbs-last` handoff remains.
- [ ] Orchestration errors are observable and cancellation is explicit.
- [ ] `Swapper` no longer depends on public methods being called in a fragile order.
- [ ] Generated shell command construction is centralized and tested for quoting edge cases.
- [ ] User-facing inputs are parsed into typed config before use.
- [ ] Rendering logic is testable without a real terminal where practical.
- [ ] `cargo fmt --all -- --check`, `cargo build --verbose`, and `cargo test --verbose` pass.
- [ ] Runtime flow has been manually smoke-tested in tmux after a release build.
- [ ] README and `AGENTS.md` match the final implementation.
