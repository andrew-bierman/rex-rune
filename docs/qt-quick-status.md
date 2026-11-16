# Qt Quick frontend status and handoff

The interactive POC is **POC_READY**, and the expanded Linux source-build parity implementation is **PARITY_VERIFIED** locally. Final all-feature and no-UI backend tests, both release builds, clean Qt build, formatting/Clippy, release shell/editor/resize/clipboard acceptance, native X11/Wayland checks and the fleet-claimed paired output benchmark passed. Owned-fork shipping and its CI decision remain pending. Work source: [parity plan](plans/2026-11-15-0824-feat-qt-feature-parity-plan.md); checked capabilities and limitations: [parity matrix](qt-parity.md).

Branch: `feat/qt-quick-milestone`. Delivery is only to the owned-fork [PR #1](https://github.com/andrew-bierman/rex-rune/pull/1), left unmerged. The remote PR still contains the earlier milestone until the final shipping pass. Fork Actions previously listed no registered workflows/runs; local checks are not CI. Do not retry Actions registration as a prerequisite.

## POC_READY — verified interactive checkpoint

This is a historical checkpoint, not final expanded-parity readiness. The matching final Qt/backend pair has been rebuilt and verified; use the separate executable and private demo command below. First verified on 2026-11-15. Artifacts: `qt/build/rex-qt` and `target/release/rex-qt-backend`. Initial private evidence `.build-tools/qt-acceptance-zyoo1zif/` passed shell output/input, exact Neovim editing/saving, server-observed resize and clipboard. Native Wayland clipboard evidence was added later. The initial one-pane/settings/mouse limitations have since been addressed as recorded in the matrix; the first checkpoint alone does not establish full parity.

Exact person-operated demo command from this checkout follows. It creates its own disposable server/session and stops only that server when the window closes. Agent-operated visible demos require a separate agent desktop/display: do not execute this on Andrew's desktop autonomously. Old demo windows are closed; do not restart or reattach their sessions.

```sh
cd /home/andrew-bierman/Code/rex-rune-qt
(
  mkdir -p .build-tools
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
      --config "$demo_dir/qt/workspace.json" \
      --endpoint "$demo_endpoint" --rex-session "$demo_session"
)
```

## Architecture and desktop safety

Standalone Qt Quick Controls with a C++ fixed-cell item is the smallest maintainable native host here. Quickshell would add a shell-host dependency without removing terminal/backend work. No Omarchy, Quickshell or Renux code/private interfaces are copied or required. Existing font licenses/attribution are retained; Qt is dynamically linked.

The optional private Rust child translates captured frontend requests to public Rune commands and snapshots. Upstream owns transport, reconnect, sessions, VT decoding, ownership and transfers. Original Rune remains the default UI. `vendor/` and `Cargo.lock` are unchanged. Only two narrowly committed core fixes exist: live-theme defaults and connection-epoch UI queue invalidation. See [upstream maintenance](qt-upstream-maintenance.md).

AGENTS.md records Andrew's workspace rule: interactive browser work belongs on an agent-owned Arch CUA desktop; another primary-desktop window or Hyprland workspace is not isolation. No autonomous primary-desktop window, focus or workspace changes. Native Qt automation uses offscreen or a disposable private compositor, private HOME/config/clipboard/bus and fake side-effect helpers. No personal server lifecycle, unrelated sessions, parent-pane input or installed applications are touched.

## Current evidence

All artifacts below are ignored machine-local files, not published logs/screenshots.

| Check | Current result / evidence |
| --- | --- |
| Locked all-feature Rust tests | PASS at `7072d78`: 382 core, 778 Rune UI, 38 Qt backend plus all integration suites; existing ignores retained. `.build-tools/final-reviewed-all-features-green.log` |
| Qt backend characterization/scoped tests | PASS: 38 at reviewed fix revision; `.build-tools/scoped-copy-backend.log` |
| Formatting and all-target/all-feature Clippy | PASS at `7072d78` with warnings denied in owned code; six unchanged Rune dependency warnings in Clippy. `.build-tools/final-reviewed-fmt.log`, `final-reviewed-clippy.log` |
| Original Rune and Qt backend release | PASS at `7072d78` under capacity leases: `.build-tools/final-reviewed-release-pair.log`, `final-reviewed-no-ui-release.log` |
| Production Qt and separate test driver | PASS clean production build (`final-reviewed-clean-qt.log`); separate driver rebuilt for scoped-copy fixes (`pair-global-clipboard-driver.log`) |
| Vendored SDK | PASS unit tests and six doctests; live-server cases remain ignored. `.build-tools/final-rexrs.log` |
| Shell/editor/resize/clipboard/navigation/reconnect | PASS with final release pair `.build-tools/qt-acceptance-uzrr4e0m/`; exact bytes and no input replay |
| Native Wayland clipboard | PASS with final release pair `.build-tools/qt-wayland-9j5xaeah/`: CLIPBOARD, PRIMARY, independent reader, exact middle-paste PTY bytes |
| Backend-child crashes | PASS six cases `.build-tools/qt-backend-loss-omccysb5/`: directory/download/workspace/active transfer/maintenance busy/preview; Settings and Escape responsive |
| Large host inventory | PASS `.build-tools/qt-inventory-u7mcegtl/`: 128 actually empty sessions, exact host/session identity with colliding labels, 640×480 controls, 34 native steps in 4.922s |
| Sustained output | PASS `.build-tools/final-claimed-performance/paired-results.json`: two adjacent 1/4-pane replicates, five seconds of 100 row updates/s/pane, exact echo and real geometry |

The QML stall was traced to repeatedly converting full cell/image matrices for bindings. Metadata is now projected once per publication; the C++ painter retains raw frames. The final benchmark held the fleet quiet-window claim plus a two-slot/2 GiB capacity lease. Two adjacent 1/4-pane replicates passed exact input/output and geometry checks: one-pane echoes 98–200 ms, four-pane echoes 87–191 ms, with approximately 100 ms polling quantization. Per-case load averages were recorded (one-minute 3.97–4.15). The claim automatically released afterward. These are software-rendered visible-text observations, not physical scanout or maximum throughput. Historical unleased timings are superseded; no controlled old/new speedup is claimed.

Immutable `Arc<Frame>` serialization is cached across metadata wakes and pruned for inactive panes. Full JSON records still repeat frame data (about 49–67 MB per measured case), and per-new-frame public link lookup remains costly. Dense active-search latency has not been separately measured.

Backend death retains last terminal frames visibly disconnected, closes stale captures, clears busy controls and labels active transfer outcomes unknown. It never restarts the child, replays input, fabricates cancellation or stops a server. Injected cases prove those paths, not general crash immunity. No visible demo will be launched on the primary desktop.

## Reproduction and next step

Build instructions: [Qt README](../qt/README.md). Use Rust 1.96.1, supplied Zig 0.17 and Qt 6.7+ (tested Qt 6.12). Task-local toolchains are ignored. Set absolute `RUSTUP_HOME`, `CARGO_HOME`, `TMPDIR` under `.build-tools`, prepend Cargo and Zig to PATH, and keep `--locked` working. Heavy commands now follow the fleet resource protocol and run through explicit Linux capacity leases; reduced job counts alone were insufficient. The final paired benchmark supersedes earlier unleased timing samples and records load.

```sh
cargo +1.96.1 build --locked --release --bin rex-rune
cargo +1.96.1 build --locked --release --no-default-features --bin rex-qt-backend
cargo +1.96.1 test --locked --all-features
cargo +1.96.1 fmt --all -- --check
cargo +1.96.1 clippy --locked --all-targets --all-features -- -D warnings
cargo +1.96.1 test --manifest-path vendor/rexrs/Cargo.toml --all-features
python3 scripts/qt-private.py
python3 scripts/qt-backend-loss-private.py
python3 scripts/qt-performance-private.py
python3 scripts/qt-inventory-private.py --backend target/release/rex-qt-backend --driver qt/build/test-driver/rex-qt-test-driver
python3 scripts/qt-wayland-private.py --cage /path/to/cage
```

The unsafe inherited-display `qt-private.py --native-wayland-smoke` mode has been removed; that script is always offscreen. Use `qt-wayland-private.py` for native checks on its own private compositor. The tracked Wayland clipboard fixture creates its own headless seat and never uses the desktop clipboard.

Remaining gates: push final commits to the owned fork and finish the PR/CI handoff without merging. The Qt search-layout lesson is captured under `docs/solutions/ui-bugs/`; browser testing is not applicable because the app has no HTTP/browser frontend. See the matrix for native and installation limitations.

Resource-protocol correction: read `~/Code/dotfiles/docs/guides/agent-resource-protocol.md` and the coordination hub AGENTS before heavy work. Another agent held a live quiet-window claim; owned unleased jobs were stopped and deferred until the claim was released. Every subsequent heavy command requires a fresh fleet-claim check plus `leasehold hold --if-unavailable fail` with truthful slots/memory. AGENTS.md now records this requirement. No host lease configuration was edited. The final benchmark owns the fleet claim and records per-case load; no host lease configuration was changed.

## Current continuation checkpoint

Canonical HEAD is `7072d78` (integration test aligned with the reviewed endpoint-consent contract). All eight actionable findings from the formal review have implementations or documentation corrections; the original review verdict remains unchanged and must be accompanied by the follow-up evidence. Maintenance deadline `2f56b94` passes 38 backend tests and the real native 30-second timeout/recovery fixture (`maintenance-private-hecec_lk`). Scoped server clipboard `b3295c2` passes the real server controls fixture (`qt-server-private-7jdzldk6`) and Pair clipboard/independent QR decoding/backend-loss checks. Terminal-scoped OSC 52 remains separately guarded; the final default acceptance replay passed all ten checks with the current debug backend (`qt-acceptance-actbvcz3`).

Native X11 verification passed on an authenticated disposable Xvfb display: all 78 main operations plus reconnect/disconnect reports, with all 24 original assertions unchanged. Evidence: `.build-tools/qt-x11-verification/native-20261115/`. This verifies Qt event handling and native X11 clipboard integration, not physical XTest keyboard layout coverage. Owned display resources were removed afterward.

An actual upstream merge candidate incorporated `fcc1974` cleanly. Both release frontends, Qt clean builds, core/backend tests and simultaneous distinct-session attachment passed. The Rune UI aggregate had one HOME-dependent path assertion; the exact failing test passed with private HOME. See the maintenance guide for precise limits; the candidate was not imported into the delivery branch.

The first aggregate exposed one stale consent integration expectation; it was corrected to assert both same-endpoint retention and replacement revocation. The complete aggregate replay passed under a two-slot/4 GiB capacity lease: `.build-tools/final-reviewed-all-features-green.log`. A clean production Qt build and formatting passed (`final-reviewed-clean-qt.log`, `final-reviewed-fmt.log`). Matching release artifacts, the final private-Wayland replay and the claimed paired measurement passed. No-UI backend tests passed 38 cases (`final-reviewed-no-ui-tests.log`). Compound capture and browser routing are complete; only owned-fork shipping/CI handoff remains. The formal review originally reported Not ready; all eight actionable findings now have corrections and follow-up evidence, without rewriting that original verdict.

The isolated browser is reserved for the user's optional cmux sign-in. No agent interaction occurs there during authentication; cloud VM availability is unproven and does not block Qt work. Machine-local credentials and environment details stay out of published evidence.
