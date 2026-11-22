# rex-tui

A terminal workspace client for [Rex](https://www.superlogical.com/rex), built
with [Ratatui](https://ratatui.rs/). Browse hosts, sessions, and tabs in a
keyboard-driven sidebar. Open shells, split and resize panes, and reattach
from another client while programs keep running on the Rex server.

![rex-tui in Foot with a host and session tree, three tabs, and split panes for a build shell, service log, and change notes](docs/images/rex-tui-demo.png)

Fictional workspace shown in Foot. [Reproduce the screenshot](tui/README.md#capture-the-demo).

`rex-tui` is the main frontend in this fork. The [Qt Quick frontend](qt/README.md)
is super alpha, with its performance work tracked separately.

## Get started

On Arch Linux or Omarchy, download the x86_64 `.pkg.tar.zst` from
[releases](https://github.com/andrew-bierman/rex-tui/releases) and install it
with `sudo pacman -U <downloaded-package>`. The package includes the client
and its matching backend. See the [build and install guide](tui/README.md)
to build it from source.

Launch `rex-tui` in Foot or another terminal. It uses your local Rex server
by default. Open a host with **Enter**, press **F2** to create a session with
a shell, then press **F6** to type. The [TUI guide](tui/README.md) covers
remote hosts, keyboard controls, and alternate profiles.

## Customize

Press **F1** to edit or reload `settings.toml`. Set the sidebar size and
position, change pane borders and headers, remap shortcuts, and choose a
theme. The chrome and default pane colors follow Foot's palette, including
the active Omarchy theme. The TUI guide includes [configuration and theme
examples](tui/README.md#settings-and-themes).

## Roadmap

### Available in the Arch preview

- Browse and fold hosts and sessions, open tabs, and switch focus between the sidebar and a shell.
- Create sessions and shell tabs, split panes right or down, resize dividers with the keyboard, and confirm before closing a tab or ending a session.
- Type and paste into the focused pane, resize panes when the terminal changes size, and leave sessions running when the client exits.
- Enable pane-only mouse selection and copy while keeping Foot's native Shift-drag selection.
- Edit and reload TOML settings, choose named themes, inherit terminal colors, customize the sidebar and pane chrome, and remap shortcuts.
- Try an opt-in input composer that edits a command locally and pastes it into the selected shell.
- Install the TUI and its matching backend from one x86_64 Arch package. The package builds in clean Arch CI and is exercised in private Foot against disposable Rex sessions.

### Toward parity with Rex for Mac

The target is parity for everyday Rex workflows in a terminal. Use the
[Mac action reference](https://www.superlogical.com/rex/docs/reference/mac-actions)
and the [Mac settings reference](https://www.superlogical.com/rex/docs/reference/mac-settings)
as the comparison. Track each workflow through a private TUI test before
marking it available. Terminal and Linux equivalents cover desktop features
such as notifications and opening links.

| Area | Remaining TUI work |
| --- | --- |
| Mouse and clipboard | Add click-to-focus, mouse divider resizing, a right-click action menu, and mouse forwarding for programs that request it. |
| Scrollback and search | Find within a pane, jump between prompts, and add keyboard copy mode with character, line, and block selection. |
| Navigation | Add a searchable command palette, a global switcher for sessions and tabs, sidebar filtering, and recent-tab history. |
| Tabs and panes | Add rename and reorder, directional focus and movement, pane zoom and balancing, close-pane and close-others actions, and support for floating panes as the shared backend gains it. |
| Activity | Show program status on tabs, group work by session or priority, and add opt-in desktop notifications and sounds. |
| Hosts and files | Manage hosts from the TUI, guide SSH setup and device pairing, and add remote path browsing, file insertion, and link opening. |
| Terminal settings | Add a theme picker and theme imports, surface configurable key sequences and modes, and support the remaining terminal display preferences where Foot can express them. |

## Upstream

This is a fork of [Rex Rune](https://github.com/shuv1337/rex-rune) with its
own rex-tui releases. The [upstream README](https://github.com/shuv1337/rex-rune#readme)
covers the Rune frontend. The [upstream sync checklist](tui/README.md#releases-and-upstream-sync)
covers verification of the two fork frontends.
