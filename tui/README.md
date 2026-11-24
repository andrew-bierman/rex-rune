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
sudo pacman -U dist/rex-tui-0.1.1-1-x86_64.pkg.tar.zst
rex-tui
```

The package installs the TUI and the matching backend from the same checkout.

## Build a portable pair

From the repository root, stage both executables from the same checkout using
the pinned Rust and Zig toolchains and Python 3:

```sh
mise exec -- python3 scripts/stage-tui-pair.py --output dist/rex-tui-pair
```

The script targets the build host by default. Pass `--target TRIPLE` for a
configured cross toolchain. Run `rex-tui` (or `rex-tui.exe`) from the staged
directory. Keep the companion `rex-qt-backend` executable beside it.

The portable artifact workflow exercises Linux and macOS on x86_64 and arm64,
and Windows on x86_64. Each job checks the package checksum and both executables
and uploads the archive with its cargo-dist manifest. Both Linux executables
must support glibc 2.35. Windows arm64 is in the distribution plan and awaits a
validation runner setup for the pinned cargo-dist tool.

## Hosts and profiles

Run `rex-tui` in Foot or another terminal. It uses
`${XDG_CONFIG_HOME:-$HOME/.config}/rex-tui/workspace.json` and defaults to the
local Rex server on Unix if the profile is absent. The native Windows command
uses `%APPDATA%/rex-tui` when `XDG_CONFIG_HOME` is unset. Use `rex-tui --config
PATH` to select a profile explicitly. Windows needs a reachable HTTPS Rex host
in the profile because the shared backend has no local Unix socket or SSH
transport there.

To connect to a remote server, add its endpoint to that profile. For example:

```json
{
  "hosts": [
    {"label": "Workshop", "endpoint": "ssh://you@workshop.example"}
  ]
}
```

For a new SSH host, verify and trust its key using OpenSSH. See the
[host guide](../docs/hosts.md#check-the-host-key) for setup and endpoint details.

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
| Alt+Shift+Left / Right | Move the active tab one place left or right |
| Alt+a | Jump to the next tab reporting attention |
| Alt+Shift+Up / Down | Move the highlighted sidebar row up or down |
| F5 | Open a shell in a new tab |
| Shift+F5 | Ask to close the active tab |
| Shift+F9 | Ask to close the focused pane or its tab if it has only one pane |
| Shift+F6 / Shift+F7 / Shift+F8 | Rename the active session, tab, or focused pane |
| F6 | Switch input between the sidebar and the terminal |
| / | Filter hosts, sessions, and known tabs while browsing the sidebar |
| F7 | Cycle pane focus |
| Ctrl+Alt+Arrow | Focus the adjacent pane in that direction |
| F8 / F9 | Split the focused pane right or down |
| Ctrl+Shift+Arrow | Move a divider next to the focused pane |
| F10 | Exit while leaving server sessions running |
| Alt+PageUp / Alt+PageDown | Scroll the active pane back or forward |
| Alt+o | Choose a visible web link in the active pane on Linux |
| Alt+End | Return the active pane to live output |
| Alt+/ | Find text in the active pane and its scrollback |

While filtering, use the arrow keys to move through results and Enter to open
the highlighted row. Press Ctrl+U to clear the query or Esc to restore the
original sidebar selection and folds. Remap `/` with `filter_sidebar` in `[keys]`.

Directional focus stops at the edge of the split layout. Remap it with
`focus_left`, `focus_right`, `focus_up`, and `focus_down` in `[keys]`.

Tab moves keep the same tab active and stop at the ends of the tab strip.
Remap them with `move_tab_left` and `move_tab_right` in `[keys]`.

**Alt+Shift+Up/Down** moves the highlighted sidebar row among its siblings.
Host order is saved in your local profile. Sessions move within their Rex
server and tabs move within their session. Rex shares session and tab order
across clients. Moving a row keeps the current workspace focused.
Remap the shortcuts with `move_sidebar_up` and `move_sidebar_down`.

Close prompts name the pane, tab, or session and explain what will stop. Press Y
to confirm, or Enter or Esc to cancel. Remap Shift+F9 with `close_pane` in `[keys]`.

Tabs marked **!** have a program waiting for input. **×** marks a reported
error. The count on each host or session includes attention tabs inside folded
rows. **Alt+a** cycles through reported attention, with waiting tabs first.
Remap it with `next_attention` in `[keys]`. These indicators depend on
programs publishing Rex status.

To receive desktop notifications in Foot when a background tab changes to a
waiting or error state, enable this in `settings.toml` and reload Settings.

```toml
[notifications]
attention = true
```

Notifications are off by default. They use Foot's OSC 777 support with its
usual notification command and focus suppression. The client emits these
requests when Foot identifies itself via `TERM`. The messages contain no
tab names, terminal output, or program messages. Existing statuses at
startup or reconnect, the current tab, and muted tabs stay quiet.

Rename prompts capture the active target. Edit the label or clear it with
Ctrl+U, then press Enter to send the change. An empty tab or pane label restores
its generated title. Press Esc to cancel a draft or return after sending it.

Resizing Foot updates visible panes owned by this client, including while the
sidebar has focus. The terminal renders text snapshots from the backend.

In Foot, select text and use its clipboard copy shortcut (Ctrl+Shift+C by
default). Press F6 to focus the shell before using Foot's paste shortcut
(Ctrl+Shift+V by default).

## Appearance

The `terminal` theme uses the colors of the terminal emulator running
`rex-tui`. Foot loads the active Omarchy palette through its configuration.
The sidebar, tab strip, status lines, and default pane colors follow that
palette. To inherit the 16 ANSI colors inside panes too, enable
[ANSI palette inheritance](#ansi-palette).

## Capture the demo

Install Foot, labwc, grim, wtype, ImageMagick, and Liberation Mono. Pass the underlying rex-tui executable
from the package or your build. The capture script renders fictional state in a
private headless compositor. It uses a fixture backend and a disposable profile.

```sh
python3 scripts/tui-demo-capture.py --binary /usr/lib/rex-tui/rex-tui
python3 scripts/tui-demo-capture.py --binary /usr/lib/rex-tui/rex-tui --scene first-run
```

Run these commands from the repository root. The images are written to
`docs/images/rex-tui-demo.png` and `docs/images/rex-tui-first-run.png`.
Use `--raw` to omit the presentation frame or `--output` for another destination,
`--font` to select a Foot font, or `--compositor` and `--library-path`
for a separate labwc build.

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

Fetch changes from `upstream` and do the merge in a clean worktree based on
`origin/master`. Send the result to the fork through a PR. The
[upstream maintenance guide](../docs/qt-upstream-maintenance.md#update-procedure)
has the worktree commands and Qt test setup.

## Settings and themes

Press **F1** for a scrollable list of your active shortcuts. Use the arrow
keys or PageUp/PageDown to browse and **Tab** to switch to Settings. In
either view, press **E** to edit the file using `VISUAL`
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
default_colors = "terminal"

[keys]
next_tab = "Alt+n"
previous_tab = "Alt+p"
split_down = "none"
```

Sidebar width is measured in columns and shrinks on narrow screens. Omit it
for automatic sizing. The startup preference applies on the next launch, if a terminal connects
before you interact. The sidebar accepts input while Rex is connecting.

Pane text and empty space inherit the terminal emulator's default foreground
and background. Explicit program colors and inverse text still apply. Set
`default_colors = "server"` to use Rex's pane defaults instead. Older backends
always supply the server colors.

Program overrides of terminal defaults via OSC 10 and OSC 11 are retained
when their effective color differs from the server default. A same-color
override is indistinguishable from the default in the backend. Whole-screen
reverse video uses the server colors for the reversed palette.

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

Named themes style the interface. Pane color settings also control how
terminal output uses the emulator palette.

### ANSI palette

To render the first 16 ANSI colors with your terminal emulator's palette,
set `ansi_palette = "terminal"` under `[panes]` and restart rex-tui.
In Foot on Omarchy, those colors come from the active Foot theme.
The default, `"server"`, uses the palette supplied by Rex. Explicit
truecolor, ANSI indices 16–255, and palette slots overridden by a shell
program via OSC 4 retain their Rex colors. An older backend falls back
to its existing RGB rendering.

## Experimental input composer

The input composer is off by default. To try it, add the following to
`settings.toml` and reload Settings.

```toml
[experimental]
input_composer = true

[keys]
compose_input = "F11"
```

In an existing settings file, add `compose_input` to its existing `[keys]`
section. The shortcut is used only when the composer is enabled.

Open the composer with a connected shell pane selected. Edit the draft
with the arrow, Home, End, Backspace, and Delete keys, or paste text from
your terminal emulator. Each tab or newline becomes a space so the draft
stays on one line. **Enter pastes the draft into the captured pane without
pressing Enter in the shell.** Press **Esc** to return to the shell after
pasting or discard an unsent draft. If the pane or connection changes,
discard the draft and reopen the composer for the new target.

## Pane-only mouse selection

To select text without including tabs, other panes, or the sidebar, add this
to `settings.toml` and reload Settings:

```toml
[selection]
mouse = true

[keys]
copy_selection = "F12"
```

Add the binding to your existing `[keys]` section, if present. Drag with the
left mouse button starting inside the pane, release, then press **F12**.
Dragging beyond a pane edge keeps the selection within the original pane.
Copy uses OSC 52 to write to your terminal emulator's clipboard. Foot
enables this by default. Hold **Shift** while selecting to use Foot's native
selection instead, including its usual copy shortcut. Mouse selection and
its copy shortcut are off by default.

When mouse capture is enabled, the wheel scrolls the pane under the pointer.
A mouse-tracking application in the selected pane receives wheel events.
Use **Alt+PageUp** or **Alt+PageDown** for local scrollback in the active
pane, including with mouse capture off. Press **Alt+End** to jump back to
live output without sending a key to the program in the pane. Plain **End**
remains available to that program. Remap these shortcuts with `scroll_up`,
`scroll_down`, and `jump_to_live_output` in `[keys]`.

On Linux, **Alt+o** shows the destinations of visible HTTP and HTTPS links,
including labeled links from terminal programs. Use **Up** or **Down** and
press **Enter** to open a destination with your desktop's browser.
With pane mouse capture enabled, **Ctrl+click** an underlined link to open it
directly. Plain drag still selects text, and **Shift** keeps Foot's native
selection. Remap the link picker with `open_links` in `[keys]`.

Press **Alt+/** to find text in the active pane. Type a query and press
**Enter**. The footer shows the match count and the pane highlights the
current result. Press **Up** for an older match or **Down** for a newer
one. **Esc** closes Find. Remap the opener with `find` in `[keys]`.

With mouse capture off, Foot can translate the wheel into Up and Down,
which may step through shell or chat history. For pane wheel scrolling
with click-to-focus and menus disabled, set `[selection] mouse = true`
and leave `[experimental] mouse_actions = false`.

### Experimental mouse focus and pane menu

Enable pane and sidebar mouse actions:

```toml
[experimental]
mouse_actions = true
```

Click inside a pane to focus it for typing. Dragging leaves focus alone.
Right-click inside a pane for **Focus pane**, **Copy selection**, **Split
right**, or **Split down**. Copy is available when you have selected text in
that pane. Focus the pane before splitting it. Click outside the menu or
press **Esc** to dismiss. Hold **Shift** to use Foot's native selection.
Mouse actions also enable pane selection and its copy shortcut, even if
`[selection] mouse = false`. Use `[selection] mouse = true` by itself for
selection and copy without click-to-focus or menus.

Click a sidebar row to open it, click its disclosure arrow to fold or
expand it, and use the wheel to scroll the list. The wheel leaves typing
focus unchanged. Use F6 to browse with the keyboard, then arrows,
PageUp/PageDown, and Enter. Mouse actions and selection are off by default.
