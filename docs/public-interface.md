# herdr-scratch Public Interface

This document describes the stable public surface of `herdr-scratch`.
Scratchpads are logical objects. The implementation surface used inside Herdr is
private and may change without changing this interface.

## CLI

```text
herdr-scratch toggle [name] [-- <command>...]
herdr-scratch open [name] [-- <command>...]
herdr-scratch daily [--vault PATH] [--date YYYY-MM-DD] [--print-path]
herdr-scratch focus [name]
herdr-scratch hide [name]
herdr-scratch close [name]
herdr-scratch list [--json]
herdr-scratch status [name] [--json]
herdr-scratch rename <old> <new>
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

[ui]
title_template = "Scratchpad:{name}"
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

[notes]
vault_auto = true
# vault_path = "/path/to/vault"
# obsidian_config = "/path/to/obsidian.json"
# daily_subdir = "Daily"
# daily_format = "YYYY-MM-DD"
# template_path = "/path/to/template.md"
# fallback_dir = ""
# editor = "nvim"
```

Supported scopes are `global`, `workspace`, and `cwd`.

Default scratchpads open in the current tab through an **overlay viewer**: the
backing terminal lives in the configured private Herdr named session, and the
viewer pane overlays the active tab while it is focused. Because the viewer is
a real pane, every Herdr key (including `ctrl+b p` for toggle) keeps working
inside it. `toggle` from within the viewer returns focus to the previous
context. `split` and `tab` remain supported placement values for existing
configurations.

Both the backing terminal and the overlay viewer header carry the scratchpad
title from `ui.title_template` (default `"Scratchpad:{name}"`), refreshed on
every show and focus, so the floating pane reads e.g. `Scratchpad:lazygit`.
A one-shot command on the default scratchpad (`toggle -- lazygit`) shows the
command's basename instead of the default name. Daily notes use
`daily:YYYY-MM-DD` on both surfaces.

The overlay viewer always fills its pane. `ui.popup` width/height are parsed
and validated but no longer drive sizing; they remain in the config for
backward-compatible round-trips.

`behavior.change_path` (default `true`) mirrors floax's `@floax change_path`:
when a bare-shell scratchpad is shown, its backing terminal runs `cd <host-cwd>`
where `<host-cwd>` is the directory of the focused pane at invoke time.
Command scratchpads (any with a launch command) are never synced, and the sync
never counts as an error if it fails. Records store the launch command as
`launch_command` in the registry (omitted for legacy records, which fall back
to their profile's command).

Showing a scratchpad is resilient: `show` starts the backing Herdr session
(`runtime.backing_session`) before opening the viewer when it is down.
`list`/`status` include `surface` (`popup`, `split`, `tab`); the `popup`
surface means the overlay viewer is active. `doctor` reports server reachability,
how many recommended keybindings are still unconfigured, and daily-note
resolution (`notes_source` of `explicit`, `auto`, or `fallback` plus the
resolved `notes_file`).

`daily` opens today's note in a global `daily` scratchpad through `$EDITOR`
(`notes.editor`, else `$VISUAL`/`$EDITOR`, else `vim`). Vault resolution is
`--vault`, then `notes.vault_path`, then auto-detect from Obsidian's
`obsidian.json` registry (plus common vault dirs) when `notes.vault_auto` is
true, then a local fallback file under `notes.fallback_dir` (default
`<state_dir>/daily`). Daily folder/format/template come from explicit
`notes.*` settings first, then the vault's `.obsidian/daily-notes.json`
(`folder`, Moment.js `format`, `template`), then defaults (`YYYY-MM-DD`,
builtin Tasks/Notes skeleton copied verbatim). Missing explicit vaults are an
error; missing auto-detected vaults fall back with a `doctor` hint.

## Registry

Registry path:

```text
$HERDR_PLUGIN_STATE_DIR/registry.json
```

When `HERDR_PLUGIN_STATE_DIR` is not set, the CLI uses the user's normal
platform data directory.

The version-2 registry stores soft pane and terminal handles plus the backing
session name, and the overlay viewer pane when one is open. Version-1 split/tab
records migrate without terminating or moving their live panes. Every command
validates handles before using them; stale records are repaired by `open` and
`toggle`. Records additionally store the launch command as an optional value, so
older registries load unchanged.

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
herdr.scratch.daily
herdr.scratch.list
herdr.scratch.doctor
```

The manifest declares internal `scratch` and `viewer` pane entrypoints for the
persistent runtime and overlay viewer (a `terminal attach` to the backing
terminal). Users should invoke public actions or CLI commands, not either
internal entrypoint.

## Recommended Keybindings

Add the bindings below to the Herdr config (`HERDR_CONFIG_FILE`,
`$XDG_CONFIG_HOME/herdr/config.toml`, or `~/.config/herdr/config.toml`), then
run `herdr server reload-config`. `doctor` reports how many are still missing.

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
key = "prefix+shift+g"
type = "plugin_action"
command = "herdr.scratch.lazygit"
description = "toggle lazygit scratchpad"

[[keys.command]]
key = "prefix+n"
type = "plugin_action"
command = "herdr.scratch.notes"
description = "toggle notes scratchpad"

[[keys.command]]
key = "prefix+d"
type = "plugin_action"
command = "herdr.scratch.daily"
description = "open daily note"
```

Apply with `herdr server reload-config` afterwards.
