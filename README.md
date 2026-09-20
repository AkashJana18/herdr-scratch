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
- An overlay viewer pane that keeps every Herdr key (including the toggle
  binding) live while the scratchpad is focused.
- `change_path` cwd-sync: opening a bare-shell scratchpad cds its terminal to
  the directory of the pane you opened it from (floax's `@floax change_path`).
- One-shot command scratchpads, such as `open lazygit -- lazygit`.
- Daily notes: `daily` opens today's Obsidian note (vault auto-detected from
  `obsidian.json`, folder/format/template read from `.obsidian/daily-notes.json`)
  in `$EDITOR`, falling back to a local daily file when no vault is found.
- Resilient viewers: opening a scratchpad auto-starts the backing Herdr session
  if it is down.
- `list`/`status` report each scratchpad's surface (`popup`/`split`/`tab`);
  `doctor` checks Herdr server reachability and whether the recommended
  keybindings are configured.
- Scoped scratchpads: `global`, `workspace`, or `cwd`.
- Reuse of existing live scratchpads to avoid duplicates.
- Versioned JSON registry with stale-handle repair on `open` and `toggle`.
- TOML configuration for defaults, profiles, launch commands, cwd behavior, and
  environment variables.
- Herdr plugin manifest with public actions for Marketplace-style installation.
- Backend adapter boundary so future Herdr surfaces can be adopted without
  changing the CLI or config.

Scratchpads require Herdr 0.7.4 or newer. Their backing terminals live in a
private `herdr-scratch` named session, so opening and hiding them does not add
tabs or panes to the workspace you are using. The viewer overlays the tab you
open it from; press the toggle keybinding again from inside it to return to
your previous context.

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

The viewer is an overlay pane, so while the scratchpad has focus you can press
the toggle keybinding again to hide it without stopping its terminal. Invoke the
toggle action from another pane to bring it back.

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
detects the current platform, downloads the matching `v1.1.0` release asset,
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
# daily_subdir = "Daily"
# daily_format = "YYYY-MM-DD"
# editor = "nvim"
```

See [docs/public-interface.md](docs/public-interface.md) for the stable public
interface.

## Commands

```text
herdr-scratch guide
herdr-scratch toggle [name] [-- <command>...]
herdr-scratch open [name] [-- <command>...]
herdr-scratch daily [--vault PATH] [--date YYYY-MM-DD] [--print-path]
herdr-scratch focus [name]
herdr-scratch hide [name]
herdr-scratch close [name]
herdr-scratch resize <up|down> [name]
herdr-scratch fullscreen [name]
herdr-scratch reset [name]
herdr-scratch setup
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

The sizing commands (`resize`, `fullscreen`, `reset`) are deprecated no-ops:
the overlay viewer always fills its pane and cannot be sized.

## Usage Examples

Open the quick-start guide:

```bash
herdr-scratch guide
```

Toggle the default scratchpad:

```bash
herdr-scratch toggle
```

The viewer is an overlay pane, so pressing `toggle` again from inside it hides
it and returns you to your previous context. This leaves the scratchpad shell
or TUI running. Running `exit` inside the scratchpad terminates it
permanently.

Open a named scratchpad:

```bash
herdr-scratch open notes
```

Open today's daily note (Obsidian vault auto-detected, else local fallback):

```bash
herdr-scratch daily
herdr-scratch daily --print-path
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

One-time onboarding: write the recommended Herdr keybindings for the plugin
actions. The write is idempotent, never overwrites the config, and skips any
key that is already in use:

```bash
herdr-scratch setup
herdr server reload-config
```

The legacy sizing commands (`resize`, `fullscreen`, `reset`) are retained as
no-ops: the overlay viewer always fills its pane and cannot be sized.

```bash
herdr-scratch resize up     # prints a deprecation note
herdr-scratch fullscreen    # prints a deprecation note
```

Bare-shell scratchpads follow the pane they were opened from by default: opening
a scratchpad from `/path/to/project` runs `cd /path/to/project` inside it, so
`behavior.change_path = true` keeps your scratchpad in your current project.
Command scratchpads (`lazygit`, etc.) never receive the `cd`. Set
`change_path = false` to disable the sync entirely.

Inspect state:

```bash
herdr-scratch list --json
herdr-scratch status notes
herdr-scratch doctor
```

Recommended Herdr keybindings (also written automatically by
`herdr-scratch setup`, which never steals keys you have already bound):

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

Scratchpad handles point to terminals in the private backing session. The
overlay viewer is only a direct-attach pane, so its process can come and go
independently of the scratchpad runtime.

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
    git tag v1.1.0
    git push origin v1.1.0
   ```

5. GitHub Actions builds release binaries, generates `checksums.txt`, and
   publishes the GitHub Release.
6. Verify a clean install:

   ```bash
    herdr plugin install AkashJana18/herdr-scratch --ref v1.1.0
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
