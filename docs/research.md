# herdr-scratch Research

This document summarizes the research that shaped the initial implementation.

## Goal

Build a persistent scratchpad manager for Herdr inspired by the Floax workflow
for tmux, while using Herdr's plugin architecture and writing the project in
Rust.

## Herdr Architecture Summary

Herdr is a terminal workspace manager with a server/client architecture. The
running Herdr server owns workspaces, tabs, panes, terminal runtimes, PTYs,
events, persistence, and the local socket API. CLI commands and plugin commands
communicate with that running server.

Key concepts:

- Workspace: top-level work area, often tied to a project or worktree.
- Tab: layout container inside a workspace.
- Pane: terminal viewport attached to a terminal runtime.
- PTY/runtime: process and terminal state owned by Herdr.
- Plugin: out-of-process executable package declared by `herdr-plugin.toml`.
- Plugin action: manifest-declared command invoked by Herdr.
- Plugin pane: manifest-declared command launched as a Herdr-managed terminal.
- Socket API: newline-delimited JSON over a local socket.

Plugins are not loaded as native Rust libraries. They are commands Herdr starts
with a controlled environment.

## Plugin Capabilities Today

Herdr plugins can:

- declare actions in `herdr-plugin.toml`
- declare event hooks
- declare managed terminal pane entrypoints
- receive invocation context as JSON
- read config/state paths from environment variables
- launch commands through plugin pane entrypoints
- open, focus, and close plugin-managed panes through Herdr APIs
- call the Herdr CLI/socket API from their own process
- persist their own state files

Herdr plugins cannot currently:

- register runtime actions dynamically
- use a native non-terminal plugin UI surface
- hide tabs from normal Herdr navigation
- detach and later reattach the exact same plugin overlay as a documented
  lifecycle operation
- rely on Herdr-managed plugin storage beyond path discovery

## Overlay Findings

Overlay behavior was the critical uncertainty.

Documentation says plugin pane placement can be `overlay`, `split`, `tab`, or
`zoomed`. Source inspection showed overlay panes are implemented as temporary
zoomed panes over the active context. Herdr restores previous focus/zoom after
the overlay pane exits.

`plugin.pane.close` delegates to the generic pane close path. Generic pane close
removes plugin pane records, removes unattached terminal state, schedules a
session save, emits close events, and shuts detached terminal runtimes.

Conclusion: current overlays are not suitable for Floax-style persistent
hide/restore semantics. A scratchpad that needs to stay alive should be modeled
as a logical scratchpad with an opaque runtime handle, not as a public overlay.

## Floax Analysis

Floax is a tmux plugin implemented with shell scripts. It keeps a named tmux
session, typically `scratch`, and displays it in a `tmux popup`. Toggling from
the popup detaches the client. Toggling from another tmux session opens or
creates the scratch session and attaches to it inside the popup.

Floax depends heavily on tmux-specific primitives:

- named sessions
- `tmux popup`
- `tmux attach-session`
- `tmux detach-client`
- `tmux send-keys`
- global tmux options and environment
- `tmux movew` for embed/pop behavior

Transferable ideas:

- named scratchpads
- toggle workflow
- current-directory awareness
- configurable default name
- menu/list command
- focus/return behavior

Ideas not to copy directly:

- tmux session as the public model
- shell-script implementation style
- popup sizing controls as core API
- moving windows between sessions as scratchpad semantics

## Floax vs Herdr

| Feature | Floax | Herdr today | Possible in plugin? | Needs Herdr core? |
| --- | --- | --- | --- | --- |
| Persistent scratchpads | tmux named session | live pane/runtime | Yes, with registry | No |
| Popup overlays | `tmux popup` | temporary overlay pane | Partially | Yes, for true hide/restore |
| Toggle | attach/detach | focus/return | Yes | No |
| Fullscreen | popup size | zoom/layout | Partially | No |
| Named sessions | tmux session name | plugin registry name | Yes | No |
| Current working directory | `pane_current_path` | context/current pane cwd | Yes | No |
| Session persistence | tmux server | Herdr runtime/session persistence | Partially | Maybe |
| Restore existing session | attach named session | focus live handle | Yes if live | No |
| Hidden tab | native popup detach | no hidden tab API | No | Yes |
| Configuration | tmux options | plugin TOML | Yes | No |
| Keyboard shortcuts | tmux binds | Herdr keybindings | Yes | No |
| Project-local config | tmux manual | plugin scope/profile | Yes | No |

## Design Decision

`herdr-scratch` should expose a backend-agnostic scratchpad API:

- `toggle`
- `open`
- `focus`
- `hide`
- `close`
- `list`
- `status`
- `send`
- `run`

The implementation should use the best Herdr primitive available behind an
adapter. The first implementation uses documented Herdr CLI/plugin-pane
operations and stores opaque handles in the registry.

## Risks

- Undocumented Herdr behavior: mitigate by depending only on documented CLI/API
  commands for public behavior.
- Runtime handle staleness: validate every handle before use.
- Working directory ambiguity: prefer Herdr invocation context, then current pane
  info, then process cwd.
- Herdr plugin lifecycle changes: keep Herdr-specific code isolated in
  `src/herdr.rs`.
- Storage migration: version config and registry from day one.
- Marketplace quality: keep manifest, README, license, and contribution docs in
  sync with implementation.

## Confidence

Can `herdr-scratch` be implemented today?

**PARTIALLY.**

A production-quality persistent scratchpad manager can be implemented using
Herdr's current plugin and pane APIs. Exact Floax popup hide/restore semantics
need additional Herdr core support.

## Recommended Next Step

Test the current CLI against a running Herdr session:

```bash
cargo build --release
herdr plugin link .
target/release/herdr-scratch doctor
target/release/herdr-scratch toggle
target/release/herdr-scratch list
```
