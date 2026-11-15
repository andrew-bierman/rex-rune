# Qt backend boundary

`rex-qt-backend` is a frontend-owned child process using the existing Rust
`HostConnection`, `TerminalSender`, Ghostty snapshot parser and `Renderer`.
It has no Rune or Qt imports and builds with `--no-default-features`. The
adapter does not reuse Rune's `Workspace`: navigation, font measurement,
painting, key translation, focus and clipboard belong to the Qt frontend.

Standard input and output carry UTF-8 JSON objects, one per line. Standard error
is diagnostic output. Every object includes `version: 1`; incompatible versions
are refused. This is a private, versioned interface for these two executables,
not a network listener. Closing stdin or sending `shutdown` drops the host
connection and leaves server sessions running.

## Responses

The first response is `{ "type": "hello", "version": 1 }`. Subsequent `state`
objects contain `connected`, `status`, the selected `session`, `window`, `pane`,
and `sessions`, `windows`, `panes` lists with `id` and `label` fields. `terminal`
contains `connected`, `owner`, `status` and `revision`, or is null before a
stream exists. Both host and terminal wake channels drive updates. Snapshots
are deduplicated and publication is capped at one state per 33 milliseconds;
input commands retain their order independently of display publication.

`frame` is null until a snapshot exists. Otherwise it carries authoritative
`cols`, `rows`, `fg`, `bg`, `cursor`, `cursor_color`, `cursor_shape`, `cells` and
`wrapped`. Each cell has `text`, `spacer_tail`, resolved `fg`/`bg`, `bold`,
`italic`, `faint`, `invisible`, `underline`, `underline_color`, `strikethrough`
and `overline`. Colors are RGB integers. A wide character occupies its leading
cell and a spacer tail; a combining sequence stays together in `text`. Qt
places these strings at measured cell coordinates without parsing VT output.

An `error` response carries `message`; it is a rejected operation, never a
request to retry input. A frontend must disable input when its child exits or
either connection state is false, retaining the last displayed frame.

## Requests

Each request has `op` and `version`:

| Operation | Additional fields |
| --- | --- |
| `select_session` | `session` |
| `select_window` | `session`, `window` |
| `select_pane` | `session`, `window`, `pane` |
| `key` | complete target plus Rex `client.key` `payload` |
| `paste` | complete target plus `text` |
| `resize` | complete target plus `cols`, `rows`, `cell_width`, `cell_height` |
| `claim` | complete target |
| `refresh`, `shutdown` | none |

A complete target is the exact selected `session`, `window` and `pane` from
the current state. Navigation in flight invalidates the old target. The
adapter checks membership in the active server view and refuses stale,
removed, hidden or disconnected targets. Resize also requires size ownership;
selection and copying never imply a claim. Qt declares the latest desired
size after a foreground or input claim has completed.

Ordinary keys pass through `Command::Key` to `client.key`, retaining Rex Lua
handling. Paste uses `Command::Paste`. Failed input is never retained for
reconnection. Startup passes no session creation request and no terminal theme
declaration, reads no Rune settings, and exposes no server lifecycle operation.

## Extension limits

This version sends one visible frame rather than a scene graph or scrollback
database. Mouse reports, graphics, links and the full workspace action/keymap
surface are not exposed. Extending that contract requires matching adapter and
frontend changes and tests. JSON serialization cost and QPainter performance
on large, fast-updating grids have not been benchmarked.
