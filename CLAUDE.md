# CLAUDE.md

@AGENTS.md

`AGENTS.md` (imported above) is the single source of truth for every coding agent. Put project rules
there; this file only adds how Claude Code applies them.

## Claude Code

- **`main` only.** Edit, commit and push on `main` in this checkout. Do not use worktrees
  (`isolation: "worktree"`, `EnterWorktree`), feature branches or pull requests, and do not start
  sessions that would. Delegated agents work in this checkout and commit nothing themselves.
- **Commit and push only when asked**, staging files by name, ending the message with the
  co-author line the session supplies, and pushing with `git push origin main`.
- **Before reporting done**: `make check` and `make test-sql test-tools`; add `make test-e2e` for UI
  changes, the plugin Gradle compile for `src-tauri/plugins/player/android` changes and the
  `live_relay` checks after touching `relay.rs` `dispatch` (commands in `AGENTS.md`, Testing notes).
  Record what ran in `docs/VERIFICATION.md`; never claim device or playback results that did not run.
- **Shell**: macOS zsh has no `timeout` command and no `PIPESTATUS`; capture exit codes with `$?`
  right after the command instead of through a pipe.
