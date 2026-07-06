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

## Data Flow

1. A user invokes a CLI command or Herdr plugin action.
2. `cli` parses the command.
3. `config` loads defaults and user overrides.
4. `registry` loads soft references to known scratchpads.
5. `scratchpad` resolves the requested name and scope from config plus Herdr
   invocation context.
6. `herdr` validates any existing runtime handle.
7. If the handle is live, `scratchpad` reuses it; otherwise it opens a new
   runtime through the adapter.
8. Registry state is updated and saved atomically.

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

The current default backend opens scratchpads as focused split panes in the
current tab. This keeps scratchpads attached to the active work context while
the registry still stores only opaque runtime handles.

Required adapter operations:

- detect Herdr availability
- read current pane context
- validate pane/runtime handles
- focus a runtime handle
- focus the previous context
- open a scratchpad runtime
- rename a scratchpad runtime
- close a scratchpad runtime
- send text
- run a command

## Lifecycle Semantics

- `toggle`: show/focus a scratchpad, or return to previous context if it is
  already active.
- `open`: create or show a scratchpad.
- `focus`: focus only if it already exists.
- `hide`: leave the scratchpad and return to previous context when possible.
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

## Current Limitations

- `hide` means "return to the previous Herdr context" when the current backend
  cannot truly hide a live scratchpad.
- There is no Herdr-managed plugin storage API; files are owned by the plugin.
- Registry handles are best-effort and must always be validated.
