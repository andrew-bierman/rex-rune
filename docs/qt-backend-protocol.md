# Qt backend boundary

`rex-qt-backend` is a frontend-owned child process using the existing Rust
`HostConnection`, `TerminalSender`, Ghostty snapshot parser and `Renderer`.
It has no Rune UI or Qt dependency and builds with `--no-default-features`. The
adapter does not reuse Rune's `Workspace`: navigation, font measurement,
painting, key translation, focus and clipboard belong to the Qt frontend.

The core currently attaches terminal streams for the active server window's
visible blocks, rather than a frontend-specific single-pane subscription. The
adapter publishes each visible terminal with normalized placements from the public
layout helper. No Rune workspace or layout parser is forked.

Standard input and output carry UTF-8 JSON objects, one per line. Standard error
is diagnostic output. Every object includes `version: 3`; incompatible versions
are refused. This is a private, versioned interface for these two executables,
not a network listener. Closing stdin or sending `shutdown` drops all client
connections and leaves server sessions running.

## Responses

The first response is `{ "type": "hello", "version": 3 }`. Subsequent `state`
objects contain `connected`, `status`, the selected `session`, `window`, `pane`,
the current `host` identity and connection `epoch`. `sessions` and `windows` carry `id` and `label`.
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

Each targeted request has `op`, `version`, `host` and the last observed `epoch`.
Version three adds mandatory host identity because independent core connections
can have equal epochs. Both child and frontend must be upgraded together. Window
visibility and shutdown are application-scoped housekeeping:

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
| `visibility` | application window `visible` |
| `keymap_reload` | current host/epoch |
| `shortcut` | `action`, `keys`, `args`, `mode`, `operation` (`bind`, `remove`, `restore`) |
| `action` | server `action`, `args`, `origin`, target, current keymap `generation` and `revision` |
| `reconnect` | visible target |
| `clear`, `reset` | focused live target |
| `workspace` | captured target, typed `action`, `args`, viewport `width` and `height` for best-fit splits |
| `refresh` | current host/epoch |
| `host` | closed catalog/navigation `action`, `args`, captured host/epoch |
| `shutdown` | none |

A complete target names `host`, `session`, `window`, `pane` and connection `epoch`.
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

`hosts` state contains the ordered saved catalog, scoped session/tab inventories,
connection state, exact history availability, navigation-pending flag, sidebar
preferences and explicit discovery results. It distinguishes background-only views
from opened sessions. `host` actions add/edit/remove saved entries, connect or
disconnect client workers, activate or navigate exact IDs, traverse history/order,
collapse rows and show the sidebar. They never start or stop a Rex server. A new
worker or endpoint gets a fresh identity; logical history survives an explicit
reconnect to the same saved endpoint, while stale IPC captures do not.

One coordinator owns the Qt settings profile and navigation correlation. Per-host
adapters retain only pane presentation/focus and close questions. Shortcuts,
workspace preferences and catalog mutations share the authoritative profile so
saving one cannot overwrite a newer session selection. Active host changes discard
old local questions and key prefixes/modes; queued input is never carried over.
Clipboard and action events name host and epoch and are accepted only for the
visible active target. Background activity remains in the public core models.

Add/Edit validation errors have `scope: "host_form"` and remain inline until the
form closes. Other errors remain application notices. A successful catalog save
emits `host_saved` after state publication. Tailnet discovery uses public core
probes only after an explicit request; discovery never saves or attaches a host.

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

This version sends the current multi-pane scene, not a scrollback database. Graphics and other remaining parity surfaces are not yet exposed.
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

The adapter lives in `qt/backend/` and imports public `rex_rune` APIs. It is not exported by the upstream library. `Cargo.toml` adds only an optional-by-selection executable target; default Rune features and dependencies are unchanged. The only downstream `src/` change is the separately documented live-theme defaults fix shared by both frontends; `vendor/` is unchanged. A separate crate would currently add workspace/lockfile maintenance without removing another upstream source edit, so it is deferred.

The adapter owns frontend selection, JSON framing, publication pacing and stale-target rejection. `HostConnection` owns protocol connections, authoritative sessions, reconnection and terminal streams; its terminal sender owns key/paste/resize delivery, and Ghostty owns VT state. No parallel connection/reconnect/parser implementation exists in Qt. Public core APIs are not a stable compatibility promise: upstream model/command/frame changes may require adapter updates. See [upstream maintenance](qt-upstream-maintenance.md).

## Preferences

State includes a stable `preferences` projection of the public Settings values, with `_defaults`, `_themes` and resolved `_appearance` metadata. `preferences` requests carry a closed editable `patch` and a frontend `request_id`. The backend merges against its latest authoritative profile, validates and atomically saves it, then publishes state before `preferences_saved` with the same ID. Errors use scope `preferences` and the request ID; a canceled/reopened dialog ignores an older acknowledgement. Hosts, navigation, shortcut overrides and consent are outside this patch. Qt rendering preferences are local; terminal theme requests use `HostCommand::Theme` and retain the core ownership rules. Terminal text and output colours remain server-owned.
