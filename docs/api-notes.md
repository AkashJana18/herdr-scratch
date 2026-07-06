# Herdr API Notes

These notes capture the Herdr API surface relevant to `herdr-scratch`.

## Primary References

- Herdr plugins: https://herdr.dev/docs/plugins/
- Herdr socket API: https://herdr.dev/docs/socket-api/
- Herdr CLI reference: https://herdr.dev/docs/cli-reference/
- Herdr Marketplace: https://herdr.dev/docs/marketplace/
- Herdr source: https://github.com/ogulcancelik/herdr

## Plugin Model

Herdr plugins are executable packages declared by `herdr-plugin.toml`.
Actions, event hooks, panes, and link handlers are declared in the manifest.
Runtime action registration is not part of Herdr plugin v1.

Important environment variables injected by Herdr for plugin commands include:

- `HERDR_PLUGIN_ID`
- `HERDR_PLUGIN_ROOT`
- `HERDR_PLUGIN_CONFIG_DIR`
- `HERDR_PLUGIN_STATE_DIR`
- `HERDR_PLUGIN_ENTRYPOINT_ID`
- `HERDR_PLUGIN_CONTEXT_JSON`
- `HERDR_BIN_PATH`
- `HERDR_SOCKET_PATH`

There is no Herdr-managed storage API in v1. `HERDR_PLUGIN_CONFIG_DIR` and
`HERDR_PLUGIN_STATE_DIR` are path discovery only.

## Manifest Surface

`herdr-scratch` declares:

- public actions: `toggle`, `open`, `list`, `doctor`
- named convenience actions: `lazygit`, `notes`
- one internal pane entrypoint: `scratch`

The pane entrypoint runs the scratchpad session process. Users should invoke
public actions or CLI commands.

## Command Resolution Notes

Herdr stores a linked plugin's `plugin_root`, but manifest command argv values
are preserved. Source inspection shows action commands are spawned from the
manifest argv with `current_dir(plugin_root)`, and pane commands are passed to
Herdr's pane launcher with a cwd that may be overridden by the caller. Herdr does
not rewrite `target/release/herdr-scratch` into an absolute path.

For that reason, this manifest must not use the built binary as a relative
program path. The current macOS/Linux manifest invokes `/bin/sh -c` and executes
`"$HERDR_PLUGIN_ROOT/target/release/herdr-scratch" ...`, using the plugin-root
environment variable Herdr injects for actions and plugin panes.

## Relevant Herdr CLI Commands

Current adapter operations use Herdr CLI commands:

```text
herdr pane current
herdr pane get <pane_id>
herdr pane rename <pane_id> <label>
herdr pane send-text <pane_id> <text>
herdr pane run <pane_id> <command>
herdr tab get <tab_id>
herdr tab focus <tab_id>
herdr plugin pane open --plugin herdr.scratch --entrypoint scratch --placement split --direction right ...
herdr plugin pane focus <pane_id>
herdr plugin pane close <pane_id>
```

Commands that return JSON use Herdr's standard response envelope. Commands that
only acknowledge success may produce no stdout; the adapter treats an empty
successful response as `Ok`.

The Herdr CLI does not expose exact `pane.focus <pane_id>` as a CLI command, but
the socket API exposes `pane.focus`. `herdr-scratch` uses that socket method
when `HERDR_SOCKET_PATH` is available so `hide` can return to the previous pane
inside the same tab.

## Relevant Socket Methods

The socket API methods underlying the Herdr CLI include:

- `pane.current`
- `pane.get`
- `pane.list`
- `pane.focus`
- `pane.send_text`
- `pane.send_input`
- `tab.get`
- `tab.focus`
- `plugin.pane.open`
- `plugin.pane.focus`
- `plugin.pane.close`

Future adapter implementations can use the socket API directly. The public
scratchpad API should not change when that happens.

## Capability Matrix

| Capability | Herdr today | herdr-scratch strategy |
| --- | --- | --- |
| Create plugin-managed terminal | Yes | Use adapter `open_scratchpad` |
| Focus plugin pane | Yes | Validate handle, then focus |
| Rename plugin pane | Yes | Apply `ui.title_template` to pane label |
| Query pane by ID | Yes | `pane.get` |
| Query tab by ID | Yes | `tab.get` through opaque focus token |
| Persist plugin state | Yes, plugin-owned files | `registry.json` |
| Store config | Yes, plugin-owned files | `config.toml` |
| Receive invocation context | Yes | `HERDR_PLUGIN_CONTEXT_JSON` |
| Know workspace/cwd | Yes, from context/current pane | Resolve scope and cwd |
| Hide Herdr tabs | No documented API | Public `hide` returns to previous context |
| Reopen exact closed PTY | No documented plugin API | Treat closed handles as stale |

## Overlay Investigation Summary

Herdr plugin panes can be opened with an overlay placement, but source inspection
showed overlays are temporary zoomed panes tracked internally by Herdr. Closing a
plugin pane delegates to the generic pane close path, removes plugin pane
records, removes unattached terminal state, and shuts detached runtimes.

Therefore, `herdr-scratch` must not promise true popup hide/restore semantics
until Herdr exposes a documented API for that lifecycle.

## Stable Contract

The public API exposes:

- scratchpad names
- scopes
- profiles
- lifecycle states
- JSON summaries

It does not expose:

- backend placement
- Herdr layout internals
- implementation-specific runtime surface names

Runtime handles in the registry are opaque implementation details.
