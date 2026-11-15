# Qt Quick frontend handoff

The alternate frontend is implemented and locally verified on `feat/qt-quick-milestone` (2026-11-15). Its work source is [the implementation plan](plans/2026-11-14-2211-feat-qt-quick-terminal-plan.md). The original Rune frontend remains the default Cargo feature.

## POC_READY — verified interactive checkpoint

Status: **POC_READY** (2026-11-15). This checkpoint is based on runtime evidence, not the earlier adapter-only report. Production artifacts are `qt/build/rex-qt` and `target/release/rex-qt-backend`.

Exact launch command for a fresh, isolated interactive demo (run in Bash from this checkout). It creates only its own private server/session and stops only that server on exit; closing the Qt window ends the demo:

```sh
cd /home/andrew-bierman/Code/rex-rune-qt
(
  demo_dir=$(mktemp -d "$PWD/.build-tools/qt-human-demo-XXXXXX")
  demo_endpoint="unix://$demo_dir/rex.sock"
  rex server run --listen "$demo_endpoint" --restore=false --config /dev/null \
    --state-dir "$demo_dir/server" >"$demo_dir/server.log" 2>&1 &
  demo_pid=$!
  trap 'kill "$demo_pid" 2>/dev/null; wait "$demo_pid" 2>/dev/null' EXIT
  for attempt in {1..100}; do
    [ -S "$demo_dir/rex.sock" ] && break
    kill -0 "$demo_pid" 2>/dev/null || exit 1
    sleep .1
  done
  demo_session=$(rex --autostart=false --server "$demo_endpoint" api call session.create \
    '{"label":"Qt POC demo","initial_windows":[{"window_label":"Shell","layout":{"block":{"flavor":"com.superlogical.terminal.shell"}}}]}' | jq -er .session_id) || exit 1
  QT_QPA_PLATFORM=wayland QT_QPA_PLATFORMTHEME=generic \
    ./qt/build/rex-qt --backend "$PWD/target/release/rex-qt-backend" \
      --endpoint "$demo_endpoint" --session "$demo_session"
)
```

Runtime evidence: `.build-tools/qt-acceptance-zyoo1zif/` contains four successful reports (`main-report.json`, `reconnect-report.json`, `wayland-report.json`, `disconnect-report.json`). Shell output/input, full-screen Neovim editing/saving (`editor.txt` contains exactly `qt-editor-save\n`), server-observed resize, and exact clipboard copy/paste all passed. Clipboard evidence uses Qt's isolated offscreen clipboard; desktop clipboard integration is still unverified. Native Wayland rendering and key input passed separately. Ten adapter tests and the 389-test core suite passed; see details below.

Remaining POC limits: one displayed pane, visible-frame copy, toolbar clipboard actions, no scrollback navigation or terminal mouse reporting, and incomplete settings/keymap parity. IME preedit, physical keyboard layouts and large-grid performance remain unverified. The installed apps and personal server were not replaced or reconfigured.

Delivery: owned-fork [PR #1](https://github.com/andrew-bierman/rex-rune/pull/1) is open and unmerged. Platform CI is blocked: GitHub lists zero Actions workflows/runs and workflow enable returns 404, despite the workflow file being present. The passing task-list check is not platform CI evidence. Resume the CI handoff once Actions registers the existing workflow; further parity work needs its own scoped plan.

## Current evidence

- Core boundary inspected: `HostConnection`, `TerminalSender` and Ghostty `Frame` have no Rune dependency. Rune workspace state and key translation are frontend-specific.
- Ten adapter tests pass, including stale targets, disconnection, owned resize, failed/superseded navigation, zoom visibility and real Ghostty wide/combining/color serialization. New navigation/zoom regressions failed before the corresponding fixes.
- Final locked no-default-features suite passes: 389 core tests plus integrations, with the intentionally live-only cases ignored. An initial sandboxed run could not bind private loopback fixtures; its unrestricted rerun passed.
- Plan document review completed with no retained findings. One additive cross-model review failed to write output due quota; the other completed, and local review covered the same concerns.
- Qt frontend and its separate event driver build successfully. Rendered offline checks fit 1024×700 and 640×480.
- Complete private acceptance passes: shell keys, exact Unicode/cursor-gap/soft-wrap copying, direct Unicode press/repeat/release events, IME commits, modified-key press/release bytes at the PTY, Neovim edit/save of exact expected bytes, PTY dimensions, minimum layout, navigation and input pause. A recoverable private-server pause proves failed input is not replayed after reconnection and new input still works. Native Wayland startup/rendering/key input passes without desktop clipboard operations.
- A real pixel assertion verifies that a block cursor covers both cells of a wide glyph; it failed before the cursor-span fix.
- Original Rune release build passes, as does the standalone no-default-features adapter release build.
- The all-feature run passed 385 core tests, 778 headless UI tests and integrations before four additional adapter regressions were added. The final core run covers all 389. Formatting and final all-target/all-feature Clippy with warnings denied pass. A behavior-preserving activity-test iterator rewrite resolves a Rust 1.100 lint; nine affected tests pass too.
- Vendored SDK all-feature checks pass: 154 tests and six doctests; ten tests requiring a live server stay ignored.
- Full code review found seven P2 issues, all fixed and independently source-validated. Supplemental verification confirms the wide cursor fix too. External adversarial providers were unavailable (budget/auth); the in-process adversarial review covered that lens and found no stale-target/data-loss defect. No actionable findings remain.

## Build environment

Qt 6.12.0, qmake6, Qt6Test, GNU make and Neovim are available. Rust was absent from PATH. Task-local Rust 1.100.0, rustfmt, Clippy and Cargo dependencies are under ignored `.build-tools/`. Zig 0.17 is selected from the user's supplied toolchain. Do not commit these caches.

For Cargo commands, prepend `.build-tools/cargo/bin` and the supplied Zig directory to PATH, set `RUSTUP_HOME` to `.build-tools/rustup` and `CARGO_HOME` to `.build-tools/cargo` using absolute paths. Use `--locked`; fetched dependencies permit `--offline` after Ghostty source acquisition.

Final logs include `.build-tools/final-core-test.log`, `final-rune-build.log`, `review-clippy.log`, `review-tests-green.log`, `review-acceptance-green.log` and `rexrs-test.log`. These machine-local logs are not published artifacts.

The corresponding checks (add `--offline` when using the populated local cache) are:

```sh
cargo build --locked --release --bin rex-rune
cargo build --locked --release --no-default-features --bin rex-qt-backend
cargo test --locked --no-default-features
cargo test --locked --all-features
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --manifest-path vendor/rexrs/Cargo.toml --all-features
```

## Reproduce runtime verification

Build both Qt targets using [the frontend instructions](../qt/README.md), then run:

```sh
python3 scripts/qt-private.py
# Additionally show a private fixture on the current Wayland compositor:
python3 scripts/qt-private.py --native-wayland-smoke
```

The script starts and stops its own disposable server and isolates HOME/config/state, offscreen input and clipboard. Its reconnect check pauses/resumes only that owned server PID. It never uses the configured personal server. The native smoke uses only the fixture's window and no clipboard operations. Final artifacts are in ignored `.build-tools/qt-acceptance-zyoo1zif/`: the four `main`, `reconnect`, `wayland`, `disconnect` reports, `editor.txt`, `chords.bin` and window-only captures. These are machine-local evidence, not published screenshots.

The first runtime failure was in the driver: it sent menu keys before the popup was ready, leaving it open over subsequent input. The driver now waits for the public opened/closed state and clicks the actual menu item. Its immediate clipboard assertion prevents that failure from masquerading as a terminal input failure. The adapter also now enforces a real publication deadline during bursts of input.

## Limits

One pane is displayed at a time; this is discovery/selection of visible server splits, not a replacement split layout. Copy covers the visible frame. Scrollback, terminal mouse reporting, graphics, links, uploads/downloads and full workspace/settings/keymap parity remain deferred. Clipboard actions are toolbar-only until removable local bindings exist. Direct Unicode events and IME commits are tested; physical keyboard-layout fidelity, IME preedit and desktop clipboard integration remain unverified. Large-grid performance is not benchmarked. The committed GitHub CI definition covers Rust, but the owned fork has not run it; the Qt runtime harness currently requires the local Qt/Rex/Neovim environment. See [the protocol boundary](qt-backend-protocol.md) and [frontend notes](../qt/README.md).

## Constraints and next step

No personal server lifecycle, configuration or existing session input has been changed. Automated runtime checks must start a disposable private server and use only its own sessions, clipboard/display and files. Do not inject keys into the parent pane or resize existing work.

Implementation, simplification, code review and local verification are complete. Delivery uses only the owned fork `andrew-bierman/rex-rune` (`origin`); the original repository is `upstream` for reference. Next: finish the owned-fork PR/CI handoff, then let the owner review the milestone and choose further parity scope. Never push or open PRs against upstream and never merge.
