# Windows Hooks Design

## Goal

Enable `hooks.json` lifecycle hooks on Windows while keeping the existing hook executor behavior:

- default shell execution stays `cmd /C`
- custom `shell_program` and `shell_args` continue to override the default shell

## Current State

The `codex-hooks` crate already discovers `hooks.json` handlers and already has a Windows code path in the command runner. The feature is still disabled on Windows because `ClaudeHooksEngine::new()` returns early with a startup warning instead of discovering handlers.

## Design

Remove the Windows-only early return from `ClaudeHooksEngine::new()` in `codex-rs/hooks/src/engine/mod.rs` so that hook discovery runs on Windows the same way it does on other platforms.

Do not change `codex-rs/hooks/src/engine/command_runner.rs`. That file already:

- uses `cmd /C` by default on Windows
- uses the configured shell when `shell_program` is provided

## Validation

Add a regression test in `codex-rs/hooks/src/engine/mod.rs` that:

- creates a temporary config layer containing `hooks.json`
- enables the engine
- asserts there are no startup warnings
- asserts a `SessionStart` preview handler is discovered

This test must fail on the current Windows behavior and pass after the guard is removed.

## Out of Scope

- changing the default Windows shell to PowerShell
- reworking hook command syntax for cross-platform portability
- re-enabling the existing `codex-core` hooks integration suite on Windows
