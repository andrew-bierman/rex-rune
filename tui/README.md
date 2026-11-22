# rex-tui

A Ratatui frontend for Rex with a persistent host, session, and tab sidebar, a tab
strip, and live terminal panes. It uses the fork's shared backend to open and
create sessions, shell tabs, and split panes on Rex servers.

## Install on Arch Linux and Omarchy

From the repository root, use mise for the pinned Rust and Zig versions and
`base-devel` for `makepkg`. Build and install the package with:

```sh
mkdir -p "$PWD/dist"
(
  export PKGDEST="$PWD/dist"
  cd tui
  mise exec -- makepkg --clean
)
sudo pacman -U dist/rex-tui-0.1.0-2-x86_64.pkg.tar.zst
rex-tui
```

The package installs the TUI and the matching backend from the same checkout.
Run `rex-tui` in Foot or another terminal. It uses
`${XDG_CONFIG_HOME:-$HOME/.config}/rex-tui/workspace.json` and defaults to the
local Rex server if the profile is absent. Use `rex-tui --config PATH` to select
a profile explicitly.

To seed a new TUI profile with your existing Qt hosts, run the following once.
It preserves a TUI profile that already exists.

```sh
config_root=${XDG_CONFIG_HOME:-"$HOME/.config"}
profile="$config_root/rex-tui/workspace.json"
if hosts=$(jq '{hosts: [.hosts[] | {label, endpoint}]}' "$config_root/rex-qt/workspace.json"); then
  mkdir -p "$config_root/rex-tui"
  (set -C; printf '%s\n' "$hosts" > "$profile")
fi
```

## Controls

| Key | Action |
| --- | --- |
| Arrows or j/k, Enter | Move through the sidebar and open the selected row |
| F2 | Create a session on the active host |
| F3 / F4 | Switch tabs |
| F5 | Open a shell in a new tab |
| F6 | Switch input between the sidebar and the terminal |
| F7 | Cycle pane focus |
| F8 / F9 | Split the focused pane right or down |
| F10 | Exit while leaving server sessions running |

Resizing Foot updates visible panes owned by this client, including while the
sidebar has focus. The terminal renders text snapshots from the backend.

## Appearance

The sidebar, tab strip, and status lines use your terminal's default colors
and ANSI palette. In Omarchy, Foot gets those colors from the active theme.
The pane contents retain the colors supplied by Rex.
