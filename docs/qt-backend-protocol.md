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

The adapter lives in `qt/backend/` and imports public `rex_rune` APIs. It is not exported by the upstream library. `Cargo.toml` adds only an optional-by-selection executable target; default Rune features and dependencies are unchanged. The downstream `src/` corrections are the separately documented live-theme defaults and connection-scoped UI queue fixes shared by both frontends; `vendor/` is unchanged. A separate crate would currently add workspace/lockfile maintenance without removing another upstream source edit, so it is deferred.

The adapter owns frontend selection, JSON framing, publication pacing and stale-target rejection. `HostConnection` owns protocol connections, authoritative sessions, reconnection and terminal streams; its terminal sender owns key/paste/resize delivery, and Ghostty owns VT state. No parallel connection/reconnect/parser implementation exists in Qt. Public core APIs are not a stable compatibility promise: upstream model/command/frame changes may require adapter updates. See [upstream maintenance](qt-upstream-maintenance.md).

## Preferences

State includes a stable `preferences` projection of the public Settings values, with `_defaults`, `_themes` and resolved `_appearance` metadata. `preferences` requests carry a closed editable `patch` and a frontend `request_id`. The backend merges against its latest authoritative profile, validates and atomically saves it, then publishes state before `preferences_saved` with the same ID. Errors use scope `preferences` and the request ID; a canceled/reopened dialog ignores an older acknowledgement. Hosts, navigation, shortcut overrides and consent are outside this patch. Qt rendering preferences are local; terminal theme requests use `HostCommand::Theme` and retain the core ownership rules. Terminal text and output colours remain server-owned.

## Activity and program presentation

State `activity` contains cached public core reports across saved hosts, coverage flags and the locally persisted `priority`/`session` view. Rows retain host/connection epoch/activity epoch/session/window/pane identity; local completion/bell receipts clear only when the tab is seen, and reconnects discard old receipts. Disconnected, muted and idle rows do not enter the Activity list; the full projection also supplies tab/header/sidebar program glyphs and session words. Core status sweep and monotonic/report-qualified ages remain authoritative; the frontend does not attach unopened sessions or poll each terminal.

The `activity` operation accepts `refresh` or a strict view change, under the active host/epoch guard. Activation navigates through the existing captured host/session/window path. Snapshot labels and all reported messages render as plain text. Program name mapping follows the MIT Rune frontend policy; Qt uses licensed bundled Nerd Font family glyphs and does not bundle Superlogical artwork. These presentation maps do not form a second terminal or connection engine.

## Directory and uploads

Closed `directory.open/edit/choose/close` operations use captured identity and the public core directory reader. State exposes only the current captured browser and its matching reply; the frontend coalesces text edits and cancels its pending edit timer on close. A choice must be a valid listed path and produces ordinary `Command::Paste` with POSIX quoting, never Enter. Recent history is saved through the backend's current authoritative Settings transaction, not a stale adapter copy.

`files.upload` accepts local paths decoded from actual Qt clipboard/drop MIME URLs. It refuses unsupported API revisions before reading files, validates the visible live target, and delegates transfer/path insertion once to the existing core. The status is explicitly requested-only: no completion, progress or cancellation result is fabricated. New commands are not unrestricted RPC, and path-browser actions require this person's local action even when a server returns a client callback.
## Server settings and pairing

The `server` state is a projection of the core's cached `HostModel.server_info`.
Connection status labels retained facts as last reported while disconnected.
Durations use the public bounded elapsed-time helper and update once per minute
only while the server page is open. Listener addresses strip URL credentials,
queries and fragments; account and authentication URLs are never projected.

All server operations require the current host and epoch. `server_read` declares
public `Identify` interest; it does not attach, create or start anything.
`server_prepare_stop` and `server_prepare_setup` require a positive `request_id`
and return `server.confirmation` with the captured target and exact command.
`server_cancel` discards it. `server_confirm` consumes the matching confirmation
before dispatch; stale or repeated confirmation cannot dispatch again. Stop uses
public `StopServer` and is available for connected Unix and SSH hosts. Setup is
available only following the core's missing-SSH-server report and invokes public
`ssh::setup::start_personal` once on a worker. Results remain scoped to the original
host and epoch. Neither success nor failure reconnects or retries automatically;
Connect and review of a suggested endpoint remain explicit person actions.

`server_pair` computes a QR matrix locally from the sanitized reported HTTPS
address, without a host command. `server_pair_close` clears it. Host, epoch,
report or connection changes clear a displayed code, including loss of the
backend child. `server_copy` accepts only an exact sanitized address still present
in the displayed listener or pairing state. `server_page_open` and
`server_page_close` control the page-local duration clock. These operations are
local frontend capabilities and are never offered to remote requesters.

## Native terminal presentation

Frames include public `cell_pixels` and static `images` placements with screen/id/generation cache keys and already decoded RGBA bytes. Qt paints this public core output and releases removed/retargeted image caches; it does not parse graphics protocols. Linux native scan identity drives physical bindings and terminal wire keys while logical text stays layout-owned. Composition preedit stays local; one committed string uses the existing paste command.
## Desktop activity delivery

Each coherent `state` publication carries `activity.rows` with its one-shot
`news`/`newsState` decision, captured host/session/window/connection epoch and
activity epoch, and local seen/mute eligibility. The native frontend consumes
this once as it reads the publication, independently of QML rendering and clock
wakes. `preferences.notifications` contains the shared notification switches,
including an explicit false `play_sounds` default. Service health and tests are
local Qt controls and expose no remote operation.

`programNotices` drains the public core's OSC notice queue exactly once. Each
event has a unique publication event number, captured connection/activity epochs,
title/body and a destination key. Current cached activity rows map its block to a
tab. `notificationSessions` supplies session-only targets when a cached tab is
not available; no additional terminal query is made. Stale epochs and removed
sessions are discarded. Connection boundaries clear queued notices and bells,
so the next connection cannot adopt old effects. Notification actions validate
the still-retained captured destination and use the existing closed host
navigation operation; activation of the native window remains best effort.

## Downloads

Person-only `download.open/start/confirm/close` operations capture the complete terminal target and delegate to public core requests. The core owns byte transfer, names, free-space/large-file questions, no-overwrite storage, cancellation and partial cleanup. `download.cancel` names a row's host/epoch/id, including another saved host's active transfer. State aggregates existing per-host transfer records; finished rows expose only Show in folder. No downloaded file is opened or run. A destination edit uses the authoritative Qt preference transaction.
