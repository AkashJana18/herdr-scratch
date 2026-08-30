# herdr-scratch Public Interface

This document describes the stable public surface of `herdr-scratch`.
Scratchpads are logical objects. The implementation surface used inside Herdr is
private and may change without changing this interface.

## CLI

```text
herdr-scratch toggle [name] [-- <command>...]
herdr-scratch open [name] [-- <command>...]
herdr-scratch focus [name]
herdr-scratch hide [name]
herdr-scratch close [name]
herdr-scratch resize <up|down> [name]
herdr-scratch fullscreen [name]
herdr-scratch reset [name]
herdr-scratch list [--json]
herdr-scratch status [name] [--json]
herdr-scratch rename <old> <new>
herdr-scratch send <name> <text>
herdr-scratch run <name> <command>
herdr-scratch doctor [--json]
herdr-scratch --version
herdr-scratch config path
herdr-scratch config init [--force]
herdr-scratch config add <name> [--scope workspace|cwd|global] [--cwd context|workspace|home|PATH] -- <command>...
herdr-scratch state path
```

Public lifecycle words are `available`, `visible`, `hidden`, `stale`,
`closed`, `unknown`, and `error`.

## Configuration

Config path:

```text
$HERDR_PLUGIN_CONFIG_DIR/config.toml
```

When `HERDR_PLUGIN_CONFIG_DIR` is not set, the CLI uses the user's normal
platform config directory.

Default config:

```toml
version = 1
default_scratchpad = "scratch"

[behavior]
toggle_returns_to_previous = true
reuse_existing = true
restore_last_cwd = true
close_confirmation = true
placement = "popup"
split_direction = "right"
change_path = true
resize_step = "5%"
fullscreen_size = "100%"

[ui]
title_template = "scratch:{name}"
status_notifications = "errors"

[ui.popup]
width = "80%"
height = "80%"

[runtime]
backing_session = "herdr-scratch"

[scope]
default = "workspace"

[profiles.default]
command = []
cwd = "context"
env = {}

[scratchpads.scratch]
profile = "default"
scope = "workspace"
```

Supported scopes are `global`, `workspace`, and `cwd`.

Default scratchpads open as 80% × 80% session-modal popups. Their persistent
terminals live in the configured private Herdr named session; the popup is a
direct-attach viewer and does not change the active tab layout. Press `ctrl+b q`
inside the popup to detach it without stopping the terminal. `split` and `tab`
remain supported placement values for existing configurations.

Popup width and height accept terminal cell counts or percentage strings from
`"1%"` through `"100%"`. Herdr clamps dimensions below its popup minimum.

`behavior.resize_step` (default `"5%"`) is the delta applied by `resize up` and
`resize down` in the dimension's own unit. `behavior.fullscreen_size` (default
`"100%"`) is the size applied by `fullscreen`. Resize and fullscreen apply to
popup scratchpads only; the size is persisted per scratchpad, and `reset`
returns it to the configured `ui.popup` width and height.

`behavior.change_path` (default `true`) mirrors floax's `@floax change_path`:
when a bare-shell popup is shown, its backing terminal runs `cd <host-cwd>`
where `<host-cwd>` is the directory of the focused pane at invoke time.
Command scratchpads (any with a launch command) are never synced, and the sync
never counts as an error if it fails. Records store the launch command as
`launch_command` in the registry (omitted for legacy records, which fall back
to their profile's command).

## Registry

Registry path:

```text
$HERDR_PLUGIN_STATE_DIR/registry.json
```

When `HERDR_PLUGIN_STATE_DIR` is not set, the CLI uses the user's normal
platform data directory.

The version-2 registry stores soft pane and terminal handles plus the backing
session name. Version-1 split/tab records migrate without terminating or moving
their live panes. Every command validates handles before using them; stale
records are repaired by `open` and `toggle`. Records additionally store the
popup size, previous (pre-fullscreen) size, and launch command as optional
values, so older registries load unchanged.

## Plugin Manifest

The repository root contains `herdr-plugin.toml`.

Published installs run `scripts/install-binary.sh` as the manifest build step.
The installer downloads the matching GitHub Release asset and installs
`$HERDR_PLUGIN_ROOT/bin/herdr-scratch`. Runtime actions and pane entrypoints
execute that installed binary.

Public action IDs:

```text
herdr.scratch.toggle
herdr.scratch.open
herdr.scratch.lazygit
herdr.scratch.notes
herdr.scratch.list
herdr.scratch.doctor
herdr.scratch.size-up
herdr.scratch.size-down
herdr.scratch.fullscreen
herdr.scratch.reset
```

The manifest declares internal `scratch` and `popup` pane entrypoints for the
persistent runtime and direct-attach viewer. Users should invoke public actions
or CLI commands, not either internal entrypoint.

## Recommended Keybindings

```toml
[[keys.command]]
key = "prefix+p"
type = "plugin_action"
command = "herdr.scratch.toggle"
description = "toggle scratchpad"

[[keys.command]]
key = "prefix+shift+p"
type = "plugin_action"
command = "herdr.scratch.list"
description = "list scratchpads"

[[keys.command]]
key = "prefix+g"
type = "plugin_action"
command = "herdr.scratch.lazygit"
description = "toggle lazygit scratchpad"

[[keys.command]]
key = "prefix+n"
type = "plugin_action"
command = "herdr.scratch.notes"
description = "toggle notes scratchpad"

[[keys.command]]
key = "prefix+="
type = "plugin_action"
command = "herdr.scratch.size-up"
description = "grow popup scratchpad"

[[keys.command]]
key = "prefix+-"
type = "plugin_action"
command = "herdr.scratch.size-down"
description = "shrink popup scratchpad"

[[keys.command]]
key = "prefix+f"
type = "plugin_action"
command = "herdr.scratch.fullscreen"
description = "toggle popup fullscreen"

[[keys.command]]
key = "prefix+r"
type = "plugin_action"
command = "herdr.scratch.reset"
description = "reset popup scratchpad size"
```

The plugin does not edit Herdr keybindings automatically.
