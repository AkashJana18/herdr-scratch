# herdr-scratch

Persistent named scratchpads for [Herdr](https://github.com/ogulcancelik/herdr).

`herdr-scratch` gives Herdr users a fast place to keep shells, notes, REPLs,
logs, and project-local scratch work alive across normal navigation. The public
interface is intentionally scratchpad-oriented: users work with names, scopes,
profiles, and lifecycle states, not Herdr implementation details.

Current platform support: macOS and Linux. Published plugin installs use
prebuilt binaries from GitHub Releases, so users do not need Rust or Cargo.

## Features

- Named scratchpads with `toggle`, `open`, `focus`, `hide`, and `close`.
- Native 80% × 80% floating popups that detach without stopping their terminal.
- `change_path` cwd-sync: opening a bare-shell popup cds its terminal to the
  directory of the pane you opened it from (floax's `@floax change_path`).
- Popup sizing: step `resize` up/down, `fullscreen` toggle, and `reset` to the
  configured size, with size persisted per scratchpad.
- One-shot command scratchpads, such as `open lazygit -- lazygit`.
- Scoped scratchpads: `global`, `workspace`, or `cwd`.
- Reuse of existing live scratchpads to avoid duplicates.
- Versioned JSON registry with stale-handle repair on `open` and `toggle`.
- TOML configuration for defaults, profiles, launch commands, cwd behavior, and
  environment variables.
- Herdr plugin manifest with public actions for Marketplace-style installation.
- Backend adapter boundary so future Herdr surfaces can be adopted without
  changing the CLI or config.

Popup scratchpads require Herdr 0.7.4 or newer. Their terminals live in a
private `herdr-scratch` named session, so opening and hiding them does not add
tabs or panes to the workspace you are using. Herdr currently supports one
session-modal popup at a time; detach the active popup with `ctrl+b q` before
opening another.

## Installation

Install from GitHub:

```bash
herdr plugin install AkashJana18/herdr-scratch
```

## Quick Start

Open the visible getting-started guide:

```bash
herdr plugin action invoke guide --plugin herdr.scratch
```

Then toggle the default persistent scratchpad:

```bash
herdr plugin action invoke toggle --plugin herdr.scratch
```

While the popup has focus, press `ctrl+b q` to hide it without stopping its
terminal. Invoke the toggle action again to bring it back.

For one-key access, add this to `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "prefix+p"
type = "plugin_action"
command = "herdr.scratch.toggle"
description = "toggle scratchpad"
```

Apply the keybinding with `herdr server reload-config`, then use
`ctrl+b p` to toggle Scratch.

During installation Herdr runs `scripts/install-binary.sh`. The installer
detects the current platform, downloads the matching `v1.0.1` release asset,
verifies the SHA256 checksum from `checksums.txt`, and installs the executable
at:

```text
$HERDR_PLUGIN_ROOT/bin/herdr-scratch
```

Supported binary platforms:

| Platform | Target |
| --- | --- |
| macOS Apple Silicon | `aarch64-apple-darwin` |
| macOS Intel | `x86_64-apple-darwin` |
| Linux x86_64 | `x86_64-unknown-linux-gnu` |

Required system tools for binary installation are `/bin/sh`, `uname`, `curl`,
`tar`, and either `sha256sum` or `shasum`.

## Local Development

Build from source:

```bash
cargo build --release
```

Link the local plugin while developing:

```bash
herdr plugin link .
```

`herdr plugin link` does not build local plugins. This repository includes a
development wrapper at `bin/herdr-scratch` that executes
`target/release/herdr-scratch`, so run `cargo build --release` before linking
or invoking plugin actions during development.

Verify setup:

```bash
target/release/herdr-scratch doctor
target/release/herdr-scratch --version
```

To test the release installer without mutating this checkout, set
`HERDR_PLUGIN_ROOT` to a temporary directory:

```bash
HERDR_PLUGIN_ROOT=/tmp/herdr-scratch-install-test scripts/install-binary.sh
/tmp/herdr-scratch-install-test/bin/herdr-scratch --version
```

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

See [docs/public-interface.md](docs/public-interface.md) for the stable public
interface.

## Commands

```text
herdr-scratch guide
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

## Usage Examples

Open the quick-start guide:

```bash
herdr-scratch guide
```

Toggle the default scratchpad:

```bash
herdr-scratch toggle
```

While the popup has focus, press `ctrl+b q` to hide it. This detaches the popup
viewer but leaves the scratchpad shell or TUI running. Running `exit` inside the
scratchpad terminates it permanently.

Open a named scratchpad:

```bash
herdr-scratch open notes
```

Open lazygit with one command:

```bash
herdr-scratch open lazygit -- lazygit
```

Persist lazygit as a configured scratchpad:

```bash
herdr-scratch config init
herdr-scratch config add lazygit -- lazygit
```

Send a command to an existing scratchpad:

```bash
herdr-scratch run notes "git status"
```

Resize a popup scratchpad (step from `behavior.resize_step`), toggle it
fullscreen, or reset it to its configured size:

```bash
herdr-scratch resize up
herdr-scratch fullscreen
herdr-scratch resize down notes
herdr-scratch reset
```

Sizing applies to popup scratchpads only; a size change persists for that
scratchpad until `reset` or a `behavior` config change.

Bare-shell popups follow the pane they were opened from by default: opening a
popup from `/path/to/project` runs `cd /path/to/project` inside it, so
`behavior.change_path = true` keeps your scratchpad in your current project.
Command scratchpads (`lazygit`, etc.) never receive the `cd`. Set
`change_path = false` to disable the sync entirely.

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
key = "prefix+-"
type = "plugin_action"
command = "herdr.scratch.size-down"
description = "shrink popup scratchpad"

[[keys.command]]
key = "prefix+="
type = "plugin_action"
command = "herdr.scratch.size-up"
description = "grow popup scratchpad"

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

Popup handles point to terminals in the private backing session. The popup is
only a direct-attach viewer, so its process can come and go independently of
the scratchpad runtime.

The current implementation uses Herdr's documented CLI/plugin-pane APIs behind
that adapter. Public commands, config, and registry lifecycle terms do not expose
which Herdr surface is used internally.

Manifest actions launch `"$HERDR_PLUGIN_ROOT/bin/herdr-scratch"`. In installed
plugins that path is the downloaded release binary. In this source checkout it
is a development wrapper that delegates to `target/release/herdr-scratch`.

## Release Process

1. Update `VERSION`, `Cargo.toml`, and `herdr-plugin.toml` to the same version.
2. Run local verification:

   ```bash
   cargo fmt --check
   cargo clippy -- -D warnings
   cargo test
   cargo build --release
   target/release/herdr-scratch --version
   target/release/herdr-scratch doctor
   ```

3. Commit the release changes.
4. Create and push a matching tag:

   ```bash
   git tag v1.0.1
   git push origin v1.0.1
   ```

5. GitHub Actions builds release binaries, generates `checksums.txt`, and
   publishes the GitHub Release.
6. Verify a clean install:

   ```bash
   herdr plugin install AkashJana18/herdr-scratch --ref v1.0.1
   ```

Design decisions and assumptions:

- `VERSION` is the release source used by the installer and workflow.
- CI fails if `VERSION`, `Cargo.toml`, or `herdr-plugin.toml` disagree.
- Linux arm64 is deferred until a reliable release target is needed.
- Windows is out of scope because the manifest currently supports Linux and
  macOS only.

See:

- [docs/architecture.md](docs/architecture.md)
- [docs/api-notes.md](docs/api-notes.md)
- [docs/research.md](docs/research.md)
- [docs/public-interface.md](docs/public-interface.md)
