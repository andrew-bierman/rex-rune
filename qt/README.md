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
./rex-qt --backend ../../target/release/rex-qt-backend --endpoint unix:///path/to/rex.sock --session SESSION_ID
```

Qt 6.7 or newer with Core, Gui, Qml, Quick and QuickControls2 development files,
qmake6 and a C++17 compiler are required. This milestone was built against Qt
6.12. The backend path defaults to `rex-qt-backend` beside the Qt executable;
use an absolute `--backend` path when launching from another directory.

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
  --endpoint unix:///private/test.sock --session PRIVATE_SESSION \
  --script /path/to/steps.json --report /path/to/report.json
```

Scripts are JSON arrays. Operations are:

- `waitText`: `text`, optional `timeout` in milliseconds (default 15000).
- `waitState`: dot-separated `field` and JSON `value`, with optional numeric `tolerance`.
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

This is a Linux first milestone, not full Rune parity. It has no split layout,
scrollback navigation, terminal mouse reporting, uploads, downloads, hyperlinks,
graphics protocols, settings/keymap editor or remote-control declarations.
Copying covers the visible frame only. Underline variants other than single
and double currently use a single line. IME commit text uses the core paste
path; composition/preedit UI and non-US physical-key fidelity are not verified.
The renderer paints fixed cells through QPainter and publishes JSON frames;
large-grid performance has not been benchmarked. Offscreen QtTest verifies the
real frontend event/render path but does not prove compositor focus, physical
Wayland keyboard layout or desktop clipboard integration.

No Omarchy, Quickshell or Renux implementation/assets are copied or required.
Qt is dynamically linked and retains its upstream licenses. Project-authored
files follow the repository's MIT license.

Native Wayland clipboard verification uses a private headless seat and its own Rex server:

```sh
python3 scripts/qt-wayland-private.py --cage /path/to/cage
```

It requires Cage with headless/pixman, PRIMARY and virtual-keyboard support, `wtype`, a C++ compiler and Qt6Gui development files. `--library-path` accepts an already extracted dependency bundle; the script installs or downloads nothing. It independently reads both clipboard channels in another Qt process and checks exact middle-paste PTY bytes. It never uses the desktop seat or an existing server.
