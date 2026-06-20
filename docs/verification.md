# Verification

## Shell Wrappers

Use Bash syntax checking for wrapper changes:

```bash
bash -n tmux-thumbs.tmux tmux-thumbs.sh tmux-thumbs-install.sh
```

`bash -n` parses the scripts without executing them, so it does not launch the
interactive installer.

Do not invoke `tmux-thumbs.sh` in automated checks unless
`target/release/thumbs` and `target/release/tmux-thumbs` already exist and their
`--version` output matches the package version in `Cargo.toml`. If either
binary is missing or stale, `tmux-thumbs.sh` intentionally opens
`tmux-thumbs-install.sh` in a tmux pane.

For a manual wrapper smoke test, first build matching release binaries:

```bash
cargo build --release
```

Then source or invoke the tmux plugin from an interactive tmux session, where an
installer pane is acceptable if the binaries fail validation.
