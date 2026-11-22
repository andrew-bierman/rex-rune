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
  BUILDDIR=$(mktemp -d)
  export BUILDDIR
  trap 'rm -rf -- "$BUILDDIR"' EXIT
  cd tui
  mise exec -- makepkg --clean
)
sudo pacman -U dist/rex-tui-0.1.0-7-x86_64.pkg.tar.zst
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
| Left / Right | Collapse or expand hosts and sessions |
| F2 | Create a session on the active host |
| Shift+F2 | Ask to close the active session |
| F3 / F4 | Switch tabs |
| F5 | Open a shell in a new tab |
| Shift+F5 | Ask to close the active tab |
| F6 | Switch input between the sidebar and the terminal |
| F7 | Cycle pane focus |
| F8 / F9 | Split the focused pane right or down |
| Ctrl+Shift+Arrow | Move a divider next to the focused pane |
| F10 | Exit while leaving server sessions running |

Close prompts name the session or tab and explain what will stop. Press Y to
confirm, or Enter or Esc to cancel.

Resizing Foot updates visible panes owned by this client, including while the
sidebar has focus. The terminal renders text snapshots from the backend.

In Foot, select text and use its clipboard copy shortcut (Ctrl+Shift+C by
default). Press F6 to focus the shell before using Foot's paste shortcut
(Ctrl+Shift+V by default).

## Appearance

The sidebar, tab strip, and status lines use your terminal's default colors
and ANSI palette. In Omarchy, Foot gets those colors from the active theme.
The pane contents retain the colors supplied by Rex.

## Releases and upstream sync

Publish TUI releases under `rex-tui-v<version>` tags in the fork. Label test
builds as prereleases. Attach the Arch package with the TUI and its private
backend built from the same revision. Use the `pkgrel` in the tag for
subsequent packages of the same version, for example `rex-tui-v0.1.0-3`.

Keep shared core and backend changes small. Before merging an upstream sync,
build the shared backend and verify both consumers against disposable Rex
sessions. Run the TUI tests and check navigation, typing, resize, and exit in
private Foot. Run the [Qt verification driver](../qt/README.md#verification-driver)
with the same backend revision. Develop Qt performance changes separately
from the TUI release branch.

## Settings and themes

Press **F1** to open Settings. Press **E** to edit the file using `VISUAL`
or `EDITOR`, **R** to reload changes you made elsewhere, or **Esc** to
close the panel. Editing creates a starter file when needed and reloads it
after the editor exits. A rejected reload leaves the current settings in use.

Frontend settings live at `${XDG_CONFIG_HOME:-$HOME/.config}/rex-tui/settings.toml`.
They are independent of `workspace.json`, which stores the backend profile.
For alternate settings use `rex-tui --settings PATH`. Use
`rex-tui --print-default-settings` to view all default shortcuts and
`rex-tui --check-settings [--settings PATH]` to validate without connecting.

```toml
theme = "night"
startup_focus = "terminal"

[sidebar]
side = "right"
width = 28

[panes]
borders = true
headers = false

[keys]
next_tab = "Alt+n"
previous_tab = "Alt+p"
split_down = "none"
```

Sidebar width is measured in columns and shrinks on narrow screens. Omit it
for automatic sizing. The startup preference applies on the next launch, if a terminal connects
before you interact. The sidebar accepts input while Rex is connecting.

Shortcut overrides accept function keys, ASCII characters, and named keys
such as `Enter`, `Tab`, `PageUp`, and `Escape`. Prefix modifiers with
`Ctrl+`, `Alt+`, `Shift+` or `Super+`. Use `"none"` to disable an
action. Shortcuts must be unique, and some chords can be reserved by the
terminal emulator or desktop.

The built-in `terminal` theme uses the terminal emulator's palette. To add
the `night` theme above, save `themes/night.toml` next to your settings file
and set any of these color roles. Unspecified roles inherit the built-in
theme. Colors accept ANSI names, RGB hex values, and `"default"` for the
terminal's default foreground or background.

```toml
[chrome]
text = "default"
background = "default"
accent = "#84dcc6"
border = "darkgray"
muted = "gray"
warning = "yellow"
selection_text = "#102625"
selection_background = "#84dcc6"
```

Theme colors apply to the interface. Shell panes display the colors supplied
by Rex.
