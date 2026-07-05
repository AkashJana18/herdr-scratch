# herdr-scratch Public Interface

This document describes the stable public surface of `herdr-scratch`.
Scratchpads are logical objects. The implementation surface used inside Herdr is
private and may change without changing this interface.

## CLI

```text
herdr-scratch toggle [name]
herdr-scratch open [name]
herdr-scratch focus [name]
herdr-scratch hide [name]
herdr-scratch close [name]
herdr-scratch list [--json]
herdr-scratch status [name] [--json]
herdr-scratch rename <old> <new>
herdr-scratch send <name> <text>
herdr-scratch run <name> <command>
herdr-scratch doctor [--json]
herdr-scratch config path
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

[ui]
title_template = "scratch:{name}"
status_notifications = "errors"

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

## Registry

Registry path:

```text
$HERDR_PLUGIN_STATE_DIR/registry.json
```

When `HERDR_PLUGIN_STATE_DIR` is not set, the CLI uses the user's normal
platform data directory.

The registry is versioned and stores soft runtime handles. Every command
validates handles before using them. Stale records are repaired by `open` and
`toggle`.

## Plugin Manifest

The repository root contains `herdr-plugin.toml`.

Public action IDs:

```text
herdr.scratch.toggle
herdr.scratch.open
herdr.scratch.list
herdr.scratch.doctor
```

The manifest also declares an internal `scratch` pane entrypoint used to run the
scratchpad session process. Users should invoke public actions or CLI commands,
not the internal entrypoint.

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
```

The plugin does not edit Herdr keybindings automatically.
