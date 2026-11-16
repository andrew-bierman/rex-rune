# Rex Qt Quick frontend

This frontend uses Qt Quick Controls and a fixed-cell terminal item. Its child
`rex-qt-backend` reuses Rex Rune's Rust connection workers and libghostty-vt.
The original Rune executable and its default Cargo feature remain independent.
The [backend protocol](../docs/qt-backend-protocol.md) documents the reusable
boundary; the [verification record](../docs/qt-quick-status.md) records checked
behavior and the remaining limitations.

Build the backend from the repository root, with Rust and Zig available:

```sh
cargo build --locked --release --no-default-features --bin rex-qt-backend
mkdir -p qt/build
cd qt/build
qmake6 ../rex-qt.pro
make -j2
./rex-qt --backend ../../target/release/rex-qt-backend --endpoint unix:///path/to/rex.sock --rex-session SESSION_ID
```

Qt 6.7 or newer with Core, Gui, Qml, Quick, QuickControls2 and DBus development files,
qmake6 and a C++17 compiler are required. This milestone was built against Qt
6.12. The backend path defaults to `rex-qt-backend` beside the Qt executable;
use an absolute `--backend` path when launching from another directory.
The GUI uses `--rex-session`: Qt reserves `--session` for desktop session
restoration, so the old spelling is rejected before a backend starts.

Saved hosts have searchable session/tab navigation, a sidebar and back/forward history.
The Session, Tab and Pane controls select existing server state. All visible server panes are shown at their authoritative split/layer placements. Closing this app leaves sessions and programs running. No
Rune preferences are read or written. Ordinary keys reach
Rex's `client.key`; clipboard paste uses the core paste command. The window
claims size when it comes forward; typing and pasting also claim through the
core. Navigation and copying do not claim size. Another client's owned size is
respected until this client claims the pane again.

Drag to select terminal text, then use Copy. Paste uses the Qt clipboard.
Terminal actions (`⋯`) contains Select all terminal text. Workspace shortcuts use Rex Rune's public layered keymap: defaults, host
configuration, Qt's read-only override file, then recorded changes. Every binding
is removable or replaceable in Settings → Shortcuts. Commands, hints and key
dispatch use that same effective map. Unconsumed keys retain `client.key` and
paired-release behavior. Modes and sequences stay in the Qt interaction layer.
Qt settings default to `$XDG_CONFIG_HOME/rex-qt/workspace.json` (or
`~/.config/rex-qt/workspace.json`); `--config PATH` selects a separate profile.
The core's sibling-file rule names the read-only override `workspace.shortcuts.json`.
Rune preferences are never imported or written by default.

Linux desktop notifications use freedesktop D-Bus asynchronously, replace each
unseen tab's previous notification, and withdraw it when seen, muted, removed or
disconnected. Program OSC notices use the same captured destinations. Clicking
navigates the captured host/session/tab and asks Qt to activate the window;
Wayland focus remains compositor-controlled because public Qt does not accept a
notification activation token. Settings offers searchable service health and an
immediate test, including failure and recovery. Activity sounds are independently
off by default, coalesce by severity and use a bounded player on a worker thread.
No service or sound test is run automatically.

Find searches the screen and scrollback in the core. The terminal menu offers
prompt navigation and history-wide selection. Shift overrides program mouse
reporting for selection. Copy-on-selection and PRIMARY are opt-in policies;
link opening requires Ctrl press/release on the same validated link.

The `+` menu, pane headers and Commands expose explicit session/tab creation,
splits, focus, zoom, move, resize, balance, rename, reorder and local tab muting.
Divider dragging sends server resize requests; geometry changes only with server
state. Close dialogs name the captured target and default to Keep running.
Close others uses the IDs captured when its dialog opened. Moving a pane to a new
tab is unavailable in the current Rex API; moving it to an existing tab is supported.
Plain right-click opens the pane menu; Shift-right-click reports to a program
that requested mouse input. Clear and Reset use the existing core commands.


## Verification driver

The separate test executable shares the production application, renderer and
input handlers. QtTest injects real window events; it does not send terminal
commands directly to the adapter. It exists only in the test build.

```sh
mkdir -p qt/build/test-driver
cd qt/build/test-driver
qmake6 ../../rex-qt.pro CONFIG+=test_driver
make -j2
QT_QPA_PLATFORM=offscreen QT_QPA_PLATFORMTHEME=generic QT_QUICK_BACKEND=software \
  ./rex-qt-test-driver --backend /absolute/path/to/rex-qt-backend \
  --endpoint unix:///private/test.sock --rex-session PRIVATE_SESSION \
  --script /path/to/steps.json --report /path/to/report.json
```

Scripts are JSON arrays. Operations are:

- `waitText`: `text`, optional `timeout` in milliseconds (default 15000).
- `waitState`: dot-separated `field` and JSON `value`, with optional numeric `tolerance`. Numeric observations may instead use strict `greaterThan` / `lessThan` bounds.
- `dragDivider`: zero-based divider `index` and desired normalized `ratio`, through real pointer events.
- `waitClipboard`: `text` substring and optional `timeout`.
- `waitProblem`: visible error `text` substring and optional `timeout`.
- `type`: ASCII `text`, sent as Qt key events.
- `key`: `key` (Return, Escape, Tab, Backspace, Delete, arrows, Home, End,
  PageUp, PageDown, Space, F1–F35 or a letter/digit), optional `mods`
  (`ctrl+shift`, `alt`, `super`). Optional `text` and `action`
  (`press`, `repeat`, `release`) inject a direct key event, including Unicode
  and an empty-text release; without an action, a press/release pair is sent.
- `ime`: commit Unicode `text` through a Qt input-method event.
- `paste`: Unicode `text`, placed in Qt clipboard and pasted with the real
  toolbar button.
- `resize`: window `width` and `height`.
- `select`: `control` (`sessions`, `tabs`, `panes`) and zero-based `index`;
  selects through the ComboBox's keyboard interaction.
- `selectAll`, `copy`: invoke the real toolbar/menu controls with pointer/key
  events.
- `snapshot`: `path` for backend state plus clipboard and Qt measured grid.
- `screenshot`: `path` for only this Qt window's rendered image.
- `assertWideCursor`: check that the active block cursor paints both cells of
  the wide glyph provided by the private fixture.
- `pause`: `ms`, for observing asynchronous state transitions.

A timeout or failed operation exits nonzero and records the last visible text.
The driver runs focused key-payload contract checks before its script. Use only
private disposable fixtures; the repository acceptance script owns server and
session creation and cleanup.

## Current limits

The planned Linux Rune source-build feature set is implemented and locally verified; the capability matrix and status handoff record the tested scope. Server settings, explicit confirmed stop/setup and local pairing QR rendering are implemented; real tailnet/remote SSH operation remains unverified.
Terminal history/search/selection, mouse reports, safe links, effective shortcuts,
workspace lifecycle and saved-host navigation are implemented and privately tested.
Underline variants, local composition/preedit, static core-decoded graphics and Linux physical-key fidelity are implemented and privately verified. A real French-layout Wayland seat is covered. Third-party IME candidate UI remains unverified. Native X11 full acceptance passed on a private authenticated Xvfb display; physical X11 keyboard layout coverage remains unverified. A private one/four-pane output benchmark records measured echo latency and snapshot traffic; see the status handoff for workload and limitations. Native CLIPBOARD/PRIMARY and middle paste have real isolated Wayland
proof; offscreen tests alone do not prove physical desktop keyboard layouts.

No Omarchy, Quickshell or Renux implementation/assets are copied or required.
Qt is dynamically linked and retains its upstream licenses. Project-authored
files follow the repository's MIT license.

Native Wayland clipboard verification uses a private headless seat and its own Rex server:

```sh
python3 scripts/qt-wayland-private.py --cage /path/to/cage
```

It requires Cage with headless/pixman, PRIMARY and virtual-keyboard support, `wtype`, a C++ compiler and Qt6Gui development files. `--library-path` accepts an already extracted dependency bundle; the script installs or downloads nothing. It independently reads both clipboard channels in another Qt process and checks exact middle-paste PTY bytes. It never uses the desktop seat or an existing server.

Native Terminal/Interface settings use an editable draft with search, page history, explicit reset and correlated save results. The existing core profile value types are saved to the separate Qt profile. Font metrics and independent padding apply to rendering, pointer/IME coordinates and owned terminal resize together. Program icon count and colour controls now drive the shared native tab/header/sidebar/Activity presentation.

The Qt resources reference the existing IBM Plex Mono Regular font from `vendor/rune/crates/rune/assets/` and Symbols Nerd Font from `resources/fonts/`; their existing license files and attribution remain authoritative. The regular bundled face may use Qt synthesis for bold/italic where a matching installed face is absent. No extra font files are copied or downloaded.

Activity reads unopened-session coverage, status, progress and ages from the public core cache. Its native list preserves captured selection across report updates, supports priority/session views and navigation, and shares local seen/mute semantics with tab/session words. Tab/header/sidebar/Activity program cues use the existing licensed Symbols Nerd Font. The MIT name-mapping policy follows Rune, while generic family glyphs intentionally differ from Superlogical's branded artwork; those artwork files and their distinct notices are not bundled by Qt. Icon count and colour policies apply consistently across these surfaces.

Insert path and Go to directory use a captured native browser; directory changes insert quoted text without Enter. Recent paths use the separate endpoint-scoped Qt profile. Local file URLs from clipboard or drag/drop use the core uploader on supported servers; API 10 refuses them visibly. Already-dispatched uploads have no public cancellation/progress/completion interface, so Qt reports only that the request was submitted. Downloads use the public core on API 13: native path/destination entry, large/free-space confirmation, progress/cancel, no-overwrite saving and folder-only opening. Cancellation, disconnect and app exit clean owned partials.

Native notification and sound acceptance uses only a fake private D-Bus service
and executable player. Build the service test with `qmake6 qt/tests/notifications.pro`
in a separate build directory, then run:

```sh
python3 scripts/qt-notifications-private.py --test-bin /path/to/notification-services-test
python3 scripts/qt-notification-window-private.py --driver /path/to/rex-qt-test-driver --backend /path/to/rex-qt-backend
```

The window fixture creates its own Rex server and sessions. Neither fixture uses
the personal notification bus, audio player, display, server or settings.

Saved-host remote navigation defaults off. Native Remote hosts settings enables only the eight view actions supported by Rune; typing, fields, key sequences, popups and menus pause navigation. Terminal input, clipboard, files, creation/destruction and dialogs are never remotely offered. Requests retain connection identity and use one-shot native authorization before the existing public-core operation.
Settings → Integrations invokes the local `rex skill` command with
`--autostart=false`, shows its JSON dry-run file-change preview, and requires
confirmation before setup or removal. The explicit conflict replacement and
session naming choices are passed to Rex; Rex retains responsibility for file
ownership. OMP uses Rex's Pi target because those agents share skill files.

Settings → Updates checks the public core's signed release metadata on demand,
keeping verified version state beside the separate Qt profile. This is guidance
for the reused Rune core, not a Qt binary update service. Update this source
checkout and rebuild Qt and its private backend together. No installer, pacman
helper, or application replacement is available from Qt. Settings → About shows
the Qt runtime and reused core identity, dependency licenses and a fixed safe
report; copying it includes no hosts, endpoints, sessions, paths or environment.

`scripts/qt-maintenance-private.py --driver <test-driver> --backend <backend>`
checks native Qt controls with software/offscreen rendering and fake local Rex
outputs in a disposable HOME. It creates no real Rex sessions and invokes no
installed client or installer. Signature acceptance/tampering is also covered by
`cargo test --locked --no-default-features --lib minisign_verification_accepts_valid`.
