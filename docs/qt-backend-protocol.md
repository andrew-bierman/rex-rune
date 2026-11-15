# Qt backend boundary

`rex-qt-backend` is a frontend-owned child process using the existing Rust
`HostConnection`, `TerminalSender`, Ghostty snapshot parser and `Renderer`.
It has no Rune or Qt imports and builds with `--no-default-features`. The
adapter does not reuse Rune's `Workspace`: navigation, font measurement,
painting, key translation, focus and clipboard belong to the Qt frontend.

The core currently attaches terminal streams for the active server window's
visible blocks, rather than a frontend-specific single-pane subscription. The
adapter publishes each visible terminal with normalized placements from the public
layout helper. No Rune workspace or layout parser is forked.

Standard input and output carry UTF-8 JSON objects, one per line. Standard error
is diagnostic output. Every object includes `version: 2`; incompatible versions
are refused. This is a private, versioned interface for these two executables,
not a network listener. Closing stdin or sending `shutdown` drops the host
connection and leaves server sessions running.

## Responses

The first response is `{ "type": "hello", "version": 2 }`. Subsequent `state`
objects contain `connected`, `status`, the selected `session`, `window`, `pane`,
and the current connection `epoch`. `sessions` and `windows` carry `id` and `label`.
`panes` are ordered by the authoritative layout and carry `id`, `label`, complete
target identity, normalized `rect`, `terminal` and `frame`. `dividers` carry the
public layout helper's containing rectangle, direction, ratio and adjacent IDs.
The top-level frame/terminal summarize the focused pane for find/actions. Stable
pane IDs preserve Qt items across frame updates; unchanged item snapshots skip repaint. `terminal`
contains `connected`, `owner`, `status` and `revision`, or is null before a
stream exists. Both host and terminal wake channels drive updates. Snapshots
are deduplicated and publication is capped at one state per 33 milliseconds;
input commands retain their order independently of display publication.

`frame` is null until a snapshot exists. Otherwise it carries authoritative
`cols`, `rows`, `fg`, `bg`, `cursor`, `cursor_color`, `cursor_shape`, `cells` and
`wrapped`. Each cell has `text`, `spacer_tail`, resolved `fg`/`bg`, `bold`,
`italic`, `faint`, `invisible`, `underline`, `underline_color`, `strikethrough`
and `overline`. Additional frame fields expose core `mouse_tracking`, search
matches/index/total, tracked selection rows/text/error/revision, and validated
link spans from `Frame::link_at`. Terminal state includes the restore generation.
Colors are RGB integers. A wide character occupies its leading
cell and a spacer tail; a combining sequence stays together in `text`. Qt
places these strings at measured cell coordinates without parsing VT output.

An `error` response carries `message`; it reports an immediate rejection or a
later host-worker failure, never a request to retry input. Host notices are
consumed once and remain visible until dismissed. A frontend must disable input when its child exits or
either connection state is false, retaining the last displayed frame.

## Requests

Each request has `op`, `version` and the last observed `epoch` (except read-only
refresh and shutdown). Version two coordinates multi-pane targeting and stale-epoch
rejection; both child and frontend must be upgraded together:

| Operation | Additional fields |
| --- | --- |
| `select_session` | `session` |
| `select_window` | `session`, `window` |
| `select_pane` | `session`, `window`, `pane` |
| `key` | complete target plus Rex `client.key` `payload` |
| `paste` | complete target plus `text` |
| `resize` | complete target plus `cols`, `rows`, `cell_width`, `cell_height` |
| `claim` | complete target |
| `scroll` | visible target plus signed `lines` |
| `search`, `search_step`, `prompt_step` | visible target plus `text` or `older` |
| `selection` | visible target, restore `generation`, request `revision`, phase and pixel/cell geometry |
| `mouse`, `wheel` | focused target plus the core pointer `payload` |
| `visibility` | current epoch plus window `visible` |
| `keymap_reload` | none beyond epoch |
| `shortcut` | `action`, `keys`, `args`, `mode`, `operation` (`bind`, `remove`, `restore`) |
| `action` | server `action`, `args`, `origin`, target, current keymap `generation` and `revision` |
| `reconnect` | visible target |
| `clear`, `reset` | focused live target |
| `workspace` | captured target, typed `action`, `args`, viewport `width` and `height` for best-fit splits |
| `refresh`, `shutdown` | none |

A complete target names `session`, `window`, `pane` and current connection `epoch`.
Typing/paste require the focused target; resize and foreground claims may name
any currently visible pane. Navigation in flight invalidates the old target. Correlated
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

The `keymap` state projects implemented client action metadata, server action metadata,
core-resolved chord components, effective bindings, modes, diagnostics and recorded
changes. Qt matches input against those parsed chords; Rune resolves defaults and
override layers. Commands and shortcut hints share that projection. The optional
`--config` path defaults to Qt's separate `rex-qt/workspace.json`; its sibling
`workspace.shortcuts.json` override file is read-only. Recorded edits are saved in
Qt's settings and never alter Rune's profile. Refresh and foregrounding reload the
override file. Server actions use the public core action command with their actual
origin. Generation/revision and target checks discard stale dispatches; failed
actions are not replayed. Core-queued client actions are validated and emitted once
after the matching state publication. Current client actions deny remote access.

This version sends the current multi-pane scene, not a scrollback database. Graphics, saved-host management and other remaining parity surfaces are not yet exposed.
Selection, formatting and scrollback/search stay in the core; Qt never parses
VT output or builds its own history database. Local viewport requests can be
issued during pauses; the core worker determines when its stream can process
them. A cached acknowledged selection can still be copied while input is paused.
OSC 52 copies are emitted once only for the focused visible pane and current
epoch; reads are never answered. PRIMARY and copy-on-selection default off.
Links open only after a Ctrl press/release on the same validated link. Extending that contract requires matching adapter and
frontend changes and tests. The workspace adapter maps a closed action enum to
public host commands and layout helpers. It accepts no arbitrary RPC method.
Creation is explicit; tab/split creation inherits CWD through core `NewTerminal`.
The `workspace` state is a pending confirmation or null; acceptance consumes its
ID once and rechecks epoch, endpoint and captured view/target. Close-others IDs
are frozen at question creation. A vanished or changed target clears the question
permanently. The frontend preserves explicitly supplied capture epochs instead of
restamping them. Tab muting changes only Qt settings. JSON serialization cost and QPainter performance
on large, fast-updating grids have not been benchmarked.

## Downstream ownership

The adapter lives in `qt/backend/` and imports public `rex_rune` APIs. It is not exported by the upstream library. `Cargo.toml` adds only an optional-by-selection executable target; default Rune features and dependencies are unchanged. There are no downstream changes in `src/` or `vendor/`. A separate crate would currently add workspace/lockfile maintenance without removing another upstream source edit, so it is deferred.

The adapter owns frontend selection, JSON framing, publication pacing and stale-target rejection. `HostConnection` owns protocol connections, authoritative sessions, reconnection and terminal streams; its terminal sender owns key/paste/resize delivery, and Ghostty owns VT state. No parallel connection/reconnect/parser implementation exists in Qt. Public core APIs are not a stable compatibility promise: upstream model/command/frame changes may require adapter updates. See [upstream maintenance](qt-upstream-maintenance.md).
