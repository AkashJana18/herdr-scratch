# herdr-scratch

Persistent named scratchpads for [Herdr](https://github.com/ogulcancelik/herdr).

`herdr-scratch` gives Herdr users a fast place to keep shells, notes, REPLs,
logs, and project-local scratch work alive across normal navigation. The public
interface is intentionally scratchpad-oriented: users work with names, scopes,
profiles, and lifecycle states, not Herdr implementation details.

## Features

- Named scratchpads with `toggle`, `open`, `focus`, `hide`, and `close`.
- Scoped scratchpads: `global`, `workspace`, or `cwd`.
- Reuse of existing live scratchpads to avoid duplicates.
- Versioned JSON registry with stale-handle repair on `open` and `toggle`.
- TOML configuration for defaults, profiles, launch commands, cwd behavior, and
  environment variables.
- Herdr plugin manifest with public actions for Marketplace-style installation.
- Backend adapter boundary so future Herdr surfaces can be adopted without
  changing the CLI or config.

## Installation

Build from source:

```bash
cargo build --release
```

Link the local plugin while developing:

```bash
herdr plugin link .
```

Verify setup:

```bash
target/release/herdr-scratch doctor
```

For a future Marketplace install, the plugin is intended to be installed from a
public GitHub repository containing `herdr-plugin.toml` at the repository root.

## Configuration

Config path:

```text
$HERDR_PLUGIN_CONFIG_DIR/config.toml
```

If `HERDR_PLUGIN_CONFIG_DIR` is not set, `herdr-scratch` uses the platform user
config directory.

Example:

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

See [docs/public-interface.md](docs/public-interface.md) for the stable public
interface.

## Commands

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

## Usage Examples

Toggle the default scratchpad:

```bash
herdr-scratch toggle
```

Open a named scratchpad:

```bash
herdr-scratch open notes
```

Send a command to an existing scratchpad:

```bash
herdr-scratch run notes "git status"
```

Inspect state:

```bash
herdr-scratch list --json
herdr-scratch status notes
herdr-scratch doctor
```

Recommended Herdr keybindings:

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

## Screenshots

Placeholder: default scratchpad toggle workflow.

Placeholder: named scratchpad list/status workflow.

Placeholder: project-local scratchpad with custom profile.

## Architecture

`herdr-scratch` is organized around stable domain objects:

- `ScratchpadId`: name plus scope.
- `Profile`: launch recipe.
- `ScratchpadRecord`: durable observed state.
- `RuntimeHandle`: opaque Herdr runtime reference.
- `Herdr` adapter: all Herdr-specific behavior is isolated behind a trait.

The current implementation uses Herdr's documented CLI/plugin-pane APIs behind
that adapter. Public commands, config, and registry lifecycle terms do not expose
which Herdr surface is used internally.

See:

- [docs/architecture.md](docs/architecture.md)
- [docs/api-notes.md](docs/api-notes.md)
- [docs/research.md](docs/research.md)
- [docs/public-interface.md](docs/public-interface.md)
