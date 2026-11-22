# rex-tui

A terminal workspace client for [Rex](https://www.superlogical.com/rex), built
with [Ratatui](https://ratatui.rs/). Browse hosts, sessions, and tabs in a
keyboard-driven sidebar. Open shells, split and resize panes, and reattach
from another client while programs keep running on the Rex server.

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

A [personal Rex server started over SSH](docs/hosts.md#ssh-setup) runs until
it is stopped or the host restarts. On Linux with systemd, logging out also
ends that server and its sessions unless lingering is enabled for the login.

## Customize

Press **F1** to edit or reload `settings.toml`. Set the sidebar size and
position, change pane borders and headers, remap shortcuts, and choose a
theme. Chrome uses the terminal palette by default, including the active
Omarchy theme in Foot. The TUI guide includes [configuration and theme
examples](tui/README.md#settings-and-themes).

## Upstream

This is a fork of [Rex Rune](https://github.com/shuv1337/rex-rune) with its
own rex-tui releases. The [upstream README](https://github.com/shuv1337/rex-rune#readme)
covers the Rune frontend. This fork also maintains a separate
[Qt Quick frontend](qt/README.md). The [upstream sync checklist](tui/README.md#releases-and-upstream-sync)
covers verification of the two fork frontends.
