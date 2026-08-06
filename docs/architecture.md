# herdr-scratch Architecture

This document describes the intended long-term architecture of `herdr-scratch`.
The key constraint is that users manage scratchpads, not Herdr panes. The
implementation surface used to display or keep a scratchpad alive is private.

## Goals

- Provide persistent named scratchpads for Herdr.
- Keep the public API stable across Herdr runtime changes.
- Avoid duplicate scratchpads when an existing one is still live.
- Make state repair predictable when Herdr resources disappear.
- Stay Marketplace-friendly: manifest-driven actions, clear docs, no hidden
  config mutation.

## Public Model

The public model has five stable concepts:

- Scratchpad: a named logical workspace such as `scratch`, `notes`, or `repl`.
- Scope: the identity boundary, currently `global`, `workspace`, or `cwd`.
- Profile: a launch recipe containing command, cwd mode, and environment.
- Lifecycle: `available`, `visible`, `hidden`, `stale`, `closed`, `unknown`, or
  `error`.
- Registry: versioned local state owned by the plugin.

The public interface must not expose the internal Herdr surface. CLI output,
configuration, registry docs, and README examples use scratchpad lifecycle terms
only.

## Module Responsibilities

- `src/cli.rs`: parses the stable CLI with Clap.
- `src/config.rs`: loads versioned TOML config and supplies defaults.
- `src/registry.rs`: loads, saves, and migrates versioned JSON state.
- `src/herdr.rs`: Herdr adapter trait and current CLI-backed implementation.
- `src/scratchpad.rs`: orchestration for toggle/open/focus/hide/close/list.
- `src/output.rs`: human and JSON output formatting.
- `herdr-plugin.toml`: Marketplace-style manifest actions and internal session
  entrypoint.
- `scripts/install-binary.sh`: install-time binary downloader for Herdr plugin
  installs.

## Data Flow

1. A user invokes a CLI command or Herdr plugin action.
2. `cli` parses the command.
3. `config` loads defaults and user overrides.
4. `registry` loads soft references to known scratchpads.
5. `scratchpad` resolves the requested name and scope from config plus Herdr
   invocation context.
6. `herdr` validates any existing runtime handle.
7. If a popup handle is live, `scratchpad` opens a direct-attach viewer in the
   invoking session. Otherwise it creates a runtime in the private backing
   session first.
8. Registry state is updated and saved atomically under a process lock.

## State Model

Registry path:

```text
$HERDR_PLUGIN_STATE_DIR/registry.json
```

Records store:

- name
- scope kind and key
- profile
- lifecycle status
- opaque runtime handle
- cwd
- timestamps
- previous focus snapshot

Runtime handles are soft references. A handle can become stale at any time if
the user closes a pane, tab, workspace, or Herdr session outside
`herdr-scratch`.

## Backend Boundary

The `Herdr` trait is the only layer that should know how a scratchpad is
represented inside Herdr. Today the adapter uses documented Herdr CLI commands.
Future implementations can use richer socket APIs or native Herdr surfaces
without changing the public CLI/config contract.

The default backend keeps scratchpad terminals in a private `herdr-scratch`
named session and displays them through session-modal popup viewers in the
invoking session. This keeps the active workspace layout unchanged while the
runtime survives viewer detach. Existing split and tab handles remain valid.

Required adapter operations:

- detect Herdr availability
- read current pane context
- validate pane/runtime handles
- focus a runtime handle
- focus the previous context
- open a scratchpad runtime
- attach a popup viewer to a backing terminal
- close the active popup without closing its backing terminal
- rename a scratchpad runtime
- close a scratchpad runtime
- send text
- run a command

## Lifecycle Semantics

- `toggle`: show a scratchpad popup, or hide it when an external caller targets
  a currently attached popup.
- `open`: create or show a scratchpad.
- `focus`: focus only if it already exists.
- `hide`: close the popup viewer while leaving the backing terminal running.
- `close`: terminate and remove the live runtime handle.
- `list` and `status`: report logical state, not backend details.

## Configuration Strategy

Config is versioned from day one. New fields should be optional with defaults.
Breaking changes require a migration path and a version bump.

Supported extension points:

- more scope kinds
- profile inheritance
- richer launch commands
- environment templating
- future display-surface preferences
- import/export of registry entries

## Testing Strategy

- Unit-test config parsing and defaults.
- Unit-test registry round trips and migrations.
- Unit-test Herdr response parsing.
- Unit-test name/scope resolution.
- Add integration tests with a fake `Herdr` adapter before testing against a
  real Herdr server.
- Manual smoke test against Herdr before publishing: link, doctor, toggle, list,
  status, send, run, close.
- Release smoke test before publishing: install into a clean plugin root,
  verify checksum-protected binary download, then run `--version` and `doctor`.

## Current Limitations

- Herdr routes all input to an open popup, so the plugin action keybinding
  cannot fire from inside it. Use Herdr's direct-attach `ctrl+b q` chord.
- Herdr supports one session-modal popup at a time per session.
- There is no Herdr-managed plugin storage API; files are owned by the plugin.
- Registry handles are best-effort and must always be validated.
