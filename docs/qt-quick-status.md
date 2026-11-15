# Qt Quick frontend handoff

The alternate frontend is being built on `feat/qt-quick-milestone`. Its work source is [the implementation plan](plans/2026-11-14-2211-feat-qt-quick-terminal-plan.md). The original Rune frontend remains the default Cargo feature.

## Current evidence

- Core boundary inspected: `HostConnection`, `TerminalSender` and Ghostty `Frame` have no Rune dependency. Rune workspace state and key translation are frontend-specific.
- Adapter implemented and committed as `b3a886e`; six focused tests pass, including stale targets, disconnection, owned resize and real Ghostty wide/combining/color serialization.
- Complete locked no-default-features suite passes outside sandbox socket restrictions. An initial sandboxed run failed because private loopback fixtures could not bind; this was rerun successfully.
- Plan document review completed with no retained findings. One additive cross-model review failed to write output due quota; the other completed, and local review covered the same concerns.
- Qt frontend and its separate event driver build successfully. Rendered offline checks fit 1024×700 and 640×480.
- Complete private acceptance passes: shell keys, Unicode clipboard copy/paste, Neovim edit/save of exact expected bytes, PTY dimensions matching the measured Qt grid, minimum layout, pane/tab/session selection, and input pause on disconnection. Native Wayland startup, rendered output and Qt window key input pass too, without touching the desktop clipboard.
- Original Rune release build passes, as does the standalone no-default-features adapter release build.
- All-feature suite passes: 385 core tests, 778 headless UI tests and the integration checks (six intentionally ignored cases overall). Formatting and all-target/all-feature Clippy with warnings denied pass. Rust 1.100 reported a pre-existing activity-test range-loop lint; its behavior-preserving iterator rewrite also passes nine affected activity tests.

## Build environment

Qt 6.12.0, qmake6, Qt6Test, GNU make and Neovim are available. Rust was absent from PATH. Task-local Rust 1.100.0, rustfmt, Clippy and Cargo dependencies are under ignored `.build-tools/`. Zig 0.17 is selected from the user's supplied toolchain. Do not commit these caches.

For Cargo commands, prepend `.build-tools/cargo/bin` and the supplied Zig directory to PATH, set `RUSTUP_HOME` to `.build-tools/rustup` and `CARGO_HOME` to `.build-tools/cargo` using absolute paths. Use `--locked`; fetched dependencies permit `--offline` after Ghostty source acquisition.

Verification logs: `.build-tools/adapter-test.log`, `.build-tools/core-test.log`, `.build-tools/release-build.log`. These machine-local logs are not published artifacts.

## Reproduce runtime verification

Build both Qt targets using [the frontend instructions](../qt/README.md), then run:

```sh
python3 scripts/qt-private.py
# Additionally show a private fixture on the current Wayland compositor:
python3 scripts/qt-private.py --native-wayland-smoke
```

The script starts and stops its own disposable server and isolates HOME/config/state, offscreen input and clipboard. It never uses the configured personal server. The native smoke uses only the fixture's window and no clipboard operations. Checked run artifacts are in ignored `.build-tools/qt-acceptance-6u6c6_wm/`: `main-report.json`, `wayland-report.json`, `disconnect-report.json`, `editor.txt` and window-only captures `editor.png`, `minimum.png`, `wayland.png`, `disconnected.png`. These paths are machine-local evidence, not published screenshots.

The first runtime failure was in the driver: it sent menu keys before the popup was ready, leaving it open over subsequent input. The driver now waits for the public opened/closed state and clicks the actual menu item. Its immediate clipboard assertion prevents that failure from masquerading as a terminal input failure. The adapter also now enforces a real publication deadline during bursts of input.

## Limits

One pane is displayed at a time; this is discovery/selection of server splits, not a replacement split layout. Copy covers the visible frame. Scrollback navigation, terminal mouse reporting, graphics, links, uploads/downloads and full workspace/settings/keymap parity remain deferred. Clipboard shortcuts are currently reserved. Native Wayland compositor startup was checked, but physical keyboard layout, non-US keys, IME preedit and desktop clipboard integration remain unverified. Large-grid renderer/IPC performance is not benchmarked. See [the protocol boundary](qt-backend-protocol.md) and [frontend notes](../qt/README.md).

## Constraints and next step

No personal server lifecycle, configuration or existing session input has been changed. Automated runtime checks must start a disposable private server and use only its own sessions, clipboard/display and files. Do not inject keys into the parent pane or resize existing work.

Next: simplify/review, then ship only to the owned fork. The fork exists as `andrew-bierman/rex-rune`; never push or open PRs against upstream and never merge.
