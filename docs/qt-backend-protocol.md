# Qt backend boundary

`rex-qt-backend` is a frontend-owned child process using the existing Rust
`HostConnection`, `TerminalSender`, Ghostty snapshot parser and `Renderer`.
It has no Rune or Qt imports and builds with `--no-default-features`. The
adapter does not reuse Rune's `Workspace`: navigation, font measurement,
painting, key translation, focus and clipboard belong to the Qt frontend.

The core currently attaches terminal streams for the active server window's
visible blocks, rather than a frontend-specific single-pane subscription. The
adapter publishes only the selected frame. That coupling is retained in this
milestone instead of refactoring the original Rune workspace behavior.

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

An `error` response carries `message`; it reports an immediate rejection or a
later host-worker failure, never a request to retry input. Host notices are
consumed once and remain visible until dismissed. A frontend must disable input when its child exits or
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
the current state. Navigation in flight invalidates the old target. Correlated
completion or failure releases that pending selection and follows the current
authoritative server focus, without replaying input. The
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

## Downstream ownership

The adapter lives in `qt/backend/` and imports public `rex_rune` APIs. It is not exported by the upstream library. `Cargo.toml` adds only an optional-by-selection executable target; default Rune features and dependencies are unchanged. There are no downstream changes in `src/` or `vendor/`. A separate crate would currently add workspace/lockfile maintenance without removing another upstream source edit, so it is deferred.

The adapter owns frontend selection, JSON framing, publication pacing and stale-target rejection. `HostConnection` owns protocol connections, authoritative sessions, reconnection and terminal streams; its terminal sender owns key/paste/resize delivery, and Ghostty owns VT state. No parallel connection/reconnect/parser implementation exists in Qt. Public core APIs are not a stable compatibility promise: upstream model/command/frame changes may require adapter updates. See [upstream maintenance](qt-upstream-maintenance.md).
