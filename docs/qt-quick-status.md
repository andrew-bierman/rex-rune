# Qt Quick frontend handoff

The interactive POC is verified; full Linux source-build feature parity is still in progress on `feat/qt-quick-milestone`. The active work source is [the parity plan](plans/2026-11-15-0824-feat-qt-feature-parity-plan.md). The [first milestone plan](plans/2026-11-14-2211-feat-qt-quick-terminal-plan.md) is historical. Rune remains the default UI; vendor trees and lockfile are unchanged. Two isolated shared-core corrections—live theme defaults and connection-scoped UI queue invalidation—are documented in the upstream maintenance guide.

Current: all planned Linux feature packets are integrated, including maintenance pages; final stability, aggregate verification, simplification/review and shipping remain. The measured QML state-conversion stall is fixed; backend-loss popup invalidation is being corrected on private fixtures. Old visible demos are closed. No autonomous primary-desktop windows, focus changes or workspace switches are allowed; browser work must follow the latest interactive agent-owned Arch CUA preference in AGENTS.md. Native Qt probes remain offscreen or on a private compositor. Later parity commits are local pending final review/shipping; the owned-fork PR below still contains the earlier milestone.

## POC_READY — verified interactive checkpoint

Status: **POC_READY** (2026-11-15). This checkpoint is based on runtime evidence, not the earlier adapter-only report. Production artifacts are `qt/build/rex-qt` and `target/release/rex-qt-backend`.

Exact launch command for a fresh, isolated interactive demo (run in Bash from this checkout). It creates only its own private server/session and stops only that server on exit; closing the Qt window ends the demo:

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

Initial checkpoint evidence: `.build-tools/qt-acceptance-zyoo1zif/` contains four successful reports (`main-report.json`, `reconnect-report.json`, `wayland-report.json`, `disconnect-report.json`). Shell output/input, full-screen Neovim editing/saving (`editor.txt` contains exactly `qt-editor-save\n`), server-observed resize, and exact clipboard copy/paste all passed. At that checkpoint clipboard evidence used Qt's isolated offscreen clipboard; native clipboard proof was added later, below. Native Wayland rendering and key input passed separately. The initial Rust/adapter checks also passed; current counts are recorded in the later checkpoints.

At the initial POC checkpoint, limits included one displayed pane, visible-frame copy, toolbar clipboard actions, no scrollback navigation or terminal mouse reporting, and incomplete settings/keymap parity. IME preedit, physical keyboard layouts and large-grid performance remain unverified. The installed apps and personal server were not replaced or reconfigured.

Delivery: owned-fork [PR #1](https://github.com/andrew-bierman/rex-rune/pull/1) is open and unmerged for the first milestone. Platform CI has not run on that fork: GitHub listed zero Actions workflows/runs despite the workflow file, and the task-list check is not platform CI. Registration is optional remote verification, not a blocker to local parity work. Continue the active parity plan before the next review/shipping pass; never push or open PRs against upstream.

## Initial milestone evidence

- Core boundary inspected: `HostConnection`, `TerminalSender` and Ghostty `Frame` have no Rune dependency. Rune workspace state and key translation are frontend-specific.
- Ten adapter tests pass, including stale targets, disconnection, owned resize, failed/superseded navigation, zoom visibility and real Ghostty wide/combining/color serialization. New navigation/zoom regressions failed before the corresponding fixes.
- The initial locked in-process library run passed 389 tests including adapter cases, plus integrations, with intentionally live-only cases ignored. Later checkpoints separate upstream core and adapter counts. An initial sandboxed run could not bind private loopback fixtures; its unrestricted rerun passed.
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

The current frontend displays the authoritative visible split/layer scene. Divider dragging and captured-target workspace lifecycle controls are implemented. Copy uses core tracked selection and can include scrollback. Scrollback/search, pointer input and safe links are implemented; general settings, graphics, files and activity remain in progress against the active plan. Removable defaults and Commands now use the effective core keymap with separate Qt persistence. Direct Unicode events and IME commits are tested; native CLIPBOARD/PRIMARY ownership and middle paste pass in an isolated Wayland fixture. Physical keyboard-layout fidelity and IME preedit remain unverified. Large-grid performance is not benchmarked. The committed GitHub CI definition covers Rust, but the owned fork has not run it; the Qt runtime harness currently requires the local Qt/Rex/Neovim environment. See [the protocol boundary](qt-backend-protocol.md) and [frontend notes](../qt/README.md).

## Constraints and next step

No personal server lifecycle, configuration or existing session input has been changed. Automated runtime checks must start a disposable private server and use only its own sessions, clipboard/display and files. Do not inject keys into the parent pane or resize existing work.

The first milestone completed implementation, simplification, code review and local verification. The subsequent user-requested full parity work remains active. Delivery uses only the owned fork `andrew-bierman/rex-rune` (`origin`); the original repository is `upstream`. Continue the active parity plan, then repeat final review and delivery gates. Never push or open PRs against upstream and never merge.

## Settled downstream architecture

User-settled on 2026-11-15: remain downstream of Rex Rune and keep Qt a thin additive UI layer so upstream improvements remain importable. The adapter now lives entirely under `qt/backend/`; upstream `src/` and `vendor/` are unchanged in the final diff. Its only build integration is an additive Cargo binary entry. Default Rune features, dependencies, lockfile and licenses remain upstream-owned. No connection/session/reconnect/VT implementation was copied or replaced. Qt owns presentation and frontend selection; existing core workers own protocol and terminal behavior.

A separate crate was evaluated and deferred: existing public core APIs already suffice, and a workspace would add Cargo/lockfile churn without reducing source changes further. Unavoidable future core fixes must be narrow, separately committed and documented; none are required now. Public Rust APIs may change upstream, and Qt has no automatic parity guarantee. [The tested update procedure and conflict hotspots](qt-upstream-maintenance.md) define the maintenance gate.

The earlier Rust 1.100 Clippy iterator cleanup was reverted to preserve upstream source exactly. That upstream-only lint may recur on newer toolchains; do not silently modify Rune UI tests to accommodate the downstream build environment.

Downstream-isolation verification: all ten adapter tests pass from `qt/backend/main.rs`; the full no-default-features run passes 379 upstream core tests plus ten adapter tests and integrations. The original Rune release rebuild and formatting pass. Full private Qt acceptance passed again with artifacts in `.build-tools/qt-acceptance-05gykclo/` (including native Wayland). `git diff upstream/master -- src vendor Cargo.lock` is empty. The fetch/merge rehearsal at isolation commit `9ed9ff8` reports `Already up to date` against upstream `8c1382a`; it did not exercise conflict resolution. Logs: `.build-tools/downstream-core.log`, `downstream-rune.log`, `downstream-acceptance.log` and `upstream-rehearsal-core.log`.

The final downstream-focused review confirmed no duplicate backend engine or unnecessary core edits. It found and fixed fresh-worktree acceptance setup (`.build-tools` now created before temporary artifacts), and clarified resize ownership wording. A fresh-root runtime check also exposed a test-fixture race: Kitty may capture the launching Return release before the modified chords. The harness permits only that exact optional leading release; all four modified press/release sequences remain exact-byte assertions.

Fresh-root acceptance passed after those fixes; its log is `.build-tools/fresh-worktree-acceptance.log` and its retained private fixture artifacts are referenced there. This run began without an artifact parent directory and passed shell/editor/resize/clipboard/navigation/reconnect checks.

## Active scope: local app and feature parity

The user requested opening the app locally and continuing to feature parity on 2026-11-15. A separate production Qt window is running against a newly created private demo server/session; its task-owned launch metadata and logs are in ignored `.build-tools/active-demo.json`. Existing user sessions and installed applications were not touched. Local build/runtime evidence is the development gate. GitHub Actions registration is optional remote verification, not a prerequisite for local use or further implementation; stop treating it as the blocker to parity work.

The active work source is the parity plan and its source capability inventory. Completion must be measured against actual upstream source-build capabilities, not just a screenshot or another single-pane demo. Keep the settled thin-downstream boundary throughout. Full parity is not yet achieved.

### Authoritative multi-pane checkpoint

The Qt canvas now renders public core placements in server layer order with stable pane identities. All visible panes receive their own frames and owned sizes; typing is limited to the selected pane. Pointer focus follows the server after acknowledgment. Private protocol version two captures connection epochs and rejects stale input after reconnection. The existing local demo remains open on its original binary while implementation proceeds.

Evidence: three new scene/focus/resize tests failed before implementation; fifteen adapter tests now pass, including nested/floating/minimized placement and stale-epoch checks. Both Qt targets and the release backend build pass. Full private runtime acceptance (including native Wayland) passes in `.build-tools/qt-acceptance-lgybt6rb/`; `scene.json` proves two live nonoverlapping pane items, pointer-targeted output and 49×27 owned terminal grids. The editor capture was visually inspected and shows Neovim beside the live shell. Existing shell/editor/resize/clipboard/disconnect/reconnect checks remain green. Logs: `.build-tools/parity-scene-{red,green,acceptance}.log`. Full parity is still in progress against [the active plan](plans/2026-11-15-0824-feat-qt-feature-parity-plan.md).

### Terminal history, selection and pointer checkpoint

Core-backed scroll/search/prompt navigation, tracked word/line/drag/autoscroll selection, acknowledged copy, mouse/wheel reporting with Shift override, safe Ctrl-release link opening and focused/visible/epoch-scoped OSC 52 copies are implemented. Copy-on-selection and PRIMARY remain default off; the settings UI will expose these policies. A cached acknowledged selection remains copyable while input is paused. New history/selection operations still depend on when the upstream data worker can service its stream; no second terminal or scrollback engine was added.

Evidence: twenty adapter/terminal tests pass. The worker observed no pre-implementation red test and reported that explicitly; post-change real-event characterization found and corrected wheel sign, half-cell selection endpoints and a substring waiter false positive. Final private terminal fixture plus parent prompt/link checks passed in `.build-tools/terminal-parity-upqirdou/`: history-wide Unicode/wrapped copy, word and line selection, autoscroll covering 73 lines against 28 viewport rows, exact SGR mouse/wheel bytes, Shift selection, search next/previous/cancel, visible OSC 52 and hidden denial. Previous prompt anchors the first marker; next returns to the newer viewport. A test-only URL handler records safe open requests without launching a browser; plain click and canceled Ctrl-click open nothing, a matching Ctrl release opens once.

Full integrated private acceptance passes in `.build-tools/qt-acceptance-t8idjibz/`, including shell/editor/resize/clipboard, native Wayland input and real reconnection. A transparent child-process probe captures a real OSC 52 record, replays it after the actual backend reconnects into a new epoch, and proves the old event cannot overwrite current clipboard content. This checks Qt's IPC-event policy; the normal real-server fixture separately proves event production and reconnect behavior. Capture files are now committed atomically to avoid a test readiness/read race.

Native Wayland CLIPBOARD and PRIMARY were independently verified with the production Qt handlers on a private headless Cage compositor using extracted task-owned dependencies, no system installation or desktop clipboard mutation. Artifact `.build-tools/private-wayland-proof/production-a685f7d3/` contains exact selection values, an independent Qt reader's two-channel read and raw PTY bytes from real middle-click PRIMARY insertion. Reproduce locally with `python3 .build-tools/private-wayland-proof/run-production.py`; the helper and extracted binaries remain ignored machine-local verification artifacts. All fixture process groups were stopped. Logs include `.build-tools/parity-terminal-{tests,parent,integrated,clippy}.log`. No core/vendor/lockfile edits were required. Full parity remains active.

The targeted backend Clippy gate also passes with upstream-pinned Rust 1.96.1 and warnings denied; the toolchain is task-local and does not change the default Rust installation.

### Shared actions and shortcuts checkpoint

Commands, effective shortcut hints, removable defaults, read-only user overrides, recorded bindings, prefix cancellation and exclusive/one-shot key modes now share Rune's public keymap resolution. Server actions retain their origin and execute once; core-queued client actions are generation/target checked and delivered once. Qt uses a separate profile and never writes the user override file. The registry declares only implemented actions; workspace lifecycle and the remaining parity surfaces are still active work.

Evidence: 22 adapter tests and six keymap tests pass; focused Qt action tests pass, including red-before-fix mode/context and hint-priority regressions. Full upstream-pinned Rust 1.96.1 all-feature verification passed 379 core and 778 Rune headless UI tests, plus adapter/keymap and existing integrations. Strict all-target/all-feature Clippy passes. Logs: `.build-tools/parity-all-features.log`, `parity-actions-clippy.log`. Existing private shell/editor/resize/reconnect and terminal-history suites also pass with the action layer.

The private shortcut fixture passed in `.build-tools/qt-keys-412rai8t/`: exact PTY bytes prove consumed prefix/action/mode keys do not leak, removed shortcuts do flow through, recorded bindings persist across app restart, the override file remains byte-identical, Commands dispatches the server action once with palette origin, and queued Find opens once. The 640×480 capture was visually inspected; the final assertion measures actual layout bounds rather than the Popup's preferred implicit content size. No upstream source, vendor or lockfile changes were needed.

A follow-up targeting regression now rejects a server action carrying an obsolete tab ID even when its pane matches the current view, and proves the current target dispatches exactly once. The new test failed before the guard and all 23 adapter tests pass afterward; targeted strict Clippy passes. Logs: `.build-tools/parity-action-window-{red,green,clippy}.log`.

### Reproducible native Wayland clipboard proof

The previously machine-local proof is now a tracked harness, `scripts/qt-wayland-private.py`, with an independent public-Qt reader in `scripts/tools/qt-wayland-clipboard.cpp`. It accepts an explicit Cage executable and optional extracted-library path, installs nothing, and creates only an owned Rex server/session and headless pixman compositor with private HOME/XDG/runtime. Compiler temporary files stay in its ignored artifact directory; all owned process groups and its short runtime directory are cleaned up.

Authoritative replay passed against the current release backend and Qt driver in `.build-tools/qt-wayland-5p2sqjzo/`: exact native CLIPBOARD and PRIMARY, exact independent cross-process reads, and exact middle-button paste bytes received by the real PTY. Command: `python3 scripts/qt-wayland-private.py --cage .build-tools/private-wayland-proof/packages/usr/bin/cage --library-path .build-tools/private-wayland-proof/packages/usr/lib`. Log: `.build-tools/parity-wayland-repro.log`. The extracted compositor bundle remains local and optional; the tracked script documents normal installed dependencies. No desktop clipboard or unrelated surface was accessed.

### Workspace lifecycle and arrangement checkpoint

Explicit native actions now create sessions/tabs/splits with core CWD inheritance, focus/zoom/swap/move panes, drag/resize/balance dividers, rename server-owned titles, reorder tabs and persist local muting. Pane headers and right-click menus share captured targets. Commands opens native direction/name/destination forms where arguments are needed. Closing defaults to Keep running, consumes its captured question once and rejects changed epochs, endpoints or views. Close others never includes objects created after the question opened. Empty sessions can explicitly receive their first tab. Move to a new tab remains unsupported by the current Rex API, matching Rune's source baseline; move to an existing tab works.

Authoritative verification: 24 adapter/terminal, six keymap and eleven workspace tests pass, with strict scoped Clippy and formatting. Worker tests witnessed six initial dispatch failures plus separate empty-session, offline-mute and default-close regressions before fixes. The added Clear/Reset conversion test also failed before routing to existing core commands. Logs: `.build-tools/parity-workspace-parent-tests.log`, `parity-workspace-clippy.log`, `parity-workspace-verbs-red.log`; worker evidence is in `workspace-{red,empty-red,offline-red,green}.log`.

Final real-server/native-controls fixture passed in `.build-tools/qt-workspace-dxt3115e/` (56 Qt event steps), including explicit attachment to an empty session, creation only after action, CWD inheritance, server geometry/arrangement, exact captured close-others behavior, mute persistence and a live Python PID surviving cancellation/stale confirmation before explicit close. Native Commands, forms, default Return cancellation, Escape cancellation, divider drag and balance were exercised at 640×480; the own-window confirmation capture was visually inspected and fits. Reproduce: `python3 scripts/qt-workspace-private.py --backend target/debug/rex-qt-backend --driver qt/build/test-driver/rex-qt-test-driver`. Log: `.build-tools/parity-workspace-runtime.log`.

The terminal suite passes again in `.build-tools/terminal-parity-cgum44s1/`, now proving plain right-click opens only the local menu while Shift-right-click delivers exact program mouse bytes. A pre-existing search test captured a frame before Return's asynchronous search step completed; it now waits for actual core match indices instead of sleeping. No production search behavior changed. Divider handles retain stable identities across frame publication and occupy only their hit strips, preserving terminal hover handling. Upstream `src/`, `vendor/` and `Cargo.lock` remain unchanged. Saved-host navigation is the next active unit; full parity is still incomplete.

### Shortcut fixture completion fix

The action/workspace simplification replay exposed a pre-existing fixture deadline: raw input capture ended after seven seconds before the UI sequence's final `z`, so the next shell command became `zpython3`. An independently rebuilt pre-simplification `709f388` Qt driver failed identically in `.build-tools/qt-keys-ezhf6mgj/`; simplified builds showed the same empty raw capture. The fixture now waits for its explicit final input with a bounded 30-second deadline, retaining the exact `raw.bin == b'z'` assertion. The complete shortcut/persistence/PTY fixture passes in `.build-tools/qt-keys-r92f_051/`. Log: `.build-tools/parity-phase-actions-keymap.log`. This changes test synchronization, not application behavior.

### Action/workspace simplification pass

The required full-team pass reviewed 2,543 substantive production lines, preserving all proof tests and captured-target contracts. It consolidated terminal action presentation, made form branches explicit, avoided unnecessary availability scans for nonmatching chords, hoisted repeated shortcut filtering and removed redundant narration comments. No connection, protocol, capture or public API semantics changed.

Both Qt executables, six C++ action tests, 24 adapter/terminal tests, six keymap tests, eleven workspace tests and strict scoped Clippy pass. Post-pass workspace runtime passed in `.build-tools/qt-workspace-s_lqejw5/`; parent replay of the complete shortcut fixture passed in `.build-tools/qt-keys-r92f_051/` after the separately committed fixture synchronization fix. Parent logs are `.build-tools/parity-phase-actions-{tests,clippy,production,driver,cpp,keymap}.log`. The next unit remains saved-host navigation.

### Saved hosts and navigation checkpoint

One thin coordinator now supervises a public core HostConnection per saved host, with a single authoritative Qt profile and one navigation correlation authority. Native sidebar, searchable host/session/tab switchers, history, server-order cycling, title resolution, catalog editing and explicit client connection controls are implemented. Duplicate host labels gain scoped endpoint detail only when needed. Endpoint replacement and a newly created worker get fresh IDs, preventing old requests from matching a reused core epoch; same-machine history survives explicit reconnect. Workspace preference saves preserve newer session/host selections. No server lifecycle or connection engine was added.

The two-real-server fixture passed in `.build-tools/qt-hosts-qvnj0sr5/` with 55 native events: equal-epoch wrong-host rejection, held/superseded navigation, exact history, persisted catalog/session/host state, client-only reconnect, invalid-form retention/cancel cleanup, live GUI shell input/output, Mouse Back/Forward and readable 640×480 captures. Tailnet error handling uses a private CLI boundary fixture; it does not probe the owner's tailnet. Thirteen adapter-layer host tests include upstream negative-state projection without spawning workers. Real SSH/tailnet-success transport acceptance remains for the setup stage; no new transport was implemented.

Final locked Rust checks pass with pinned 1.96.1: 379 upstream core tests, 778 Rune UI tests, 25 backend tests, 13 host tests, seven keymap tests, twelve workspace tests and existing integrations. Seven C++ action stages, both Qt builds, release backend, formatting and all-target/all-feature strict Clippy pass. Logs: `.build-tools/parity-hosts-{all-features,final-tests,all-clippy,fmt,cpp,production,release}.log`. Four obsolete adapter-navigation tests moved to the coordinator contract; the duplicate pending-navigation implementation was removed.

Integrated shell/editor/resize/clipboard/reconnect acceptance passes in `.build-tools/qt-acceptance-fa1nh1r4/`. Workspace replay passes in `.build-tools/qt-workspace-ro6h78jb/`, including explicit new-tab creation in an externally created empty session. Native Wayland CLIPBOARD/PRIMARY and exact middle-paste PTY bytes pass with host-scoped IPC in `.build-tools/qt-wayland-cuhbqhed/`. The headless fixture now observes a bound keyboard and a real nonzero serial before claiming selection; a local clipboard cache alone is never counted as native proof. The empty-session fixture waits for actual inventory discovery before selecting it.

Private IPC is now version three and requires host identity on targeted requests/events. Window visibility is application housekeeping and does not depend on a server epoch. Qt consumes `--session` before custom parsing; the GUI now uses `--rex-session` and rejects the old spelling before Qt/backend startup. A minimal Qt probe demonstrated the swallowed argument, the private workspace replay proves the correct target, and `.build-tools/parity-hosts-cli.log` records pre-attachment rejection. The Rust child retains `--session`. The demo command above uses a fresh private profile to avoid attaching any previously saved host.

Next: implement general native settings, typography and interface controls from the active plan, then continue the remaining parity units. Upstream `src/`, `vendor/` and `Cargo.lock` remain byte-identical to the tracking baseline. The existing earlier demo window remains on its original loaded binary/session; no unrelated session or installed app was changed.

## Native settings integration

The current uncommitted settings work adds native Terminal/Interface settings with search, draft/reset/cancel and correlated save acknowledgements; shared core settings remain authoritative. Font/fallback/spacing/line-height and independent padding feed the live terminal metrics, pointer coordinates, IME rectangle and ownership-checked resize. Cursor defaults and theme declarations go through public core commands. Follow server clears prior declarations, including after restart. The settings projection cannot change hosts or navigation, and rejected preferences leave the saved profile intact.

Production and test-driver Qt targets build. Three focused preferences tests and the 25 backend tests pass. The full pinned Rust suite passes, including 380 core tests and 778 Rune UI tests (existing ignores retained); all-target/all-feature Clippy and formatting pass. Private runtime `.build-tools/preferences-private-3dh3kxgs/` passes 66 settings steps, reload checks and 14 second-client ownership steps. It proves live theme clearing equals a fresh attachment, owned metric changes reach the PTY, independent padding gives exact mouse coordinates, and another client's 81×24 grid survives font changes. The 640×480 settings capture was visually inspected and has no horizontal overflow. The final expanded runtime run `.build-tools/preferences-private-11ah9jvd/` passes 87 settings steps, five reload steps and 14 ownership steps. Individual real-font typography, bar-cursor and selection previews were visually inspected; the line-height edit also reached live metrics. C++ action tests pass all seven stages. Font resources reference the existing licensed IBM Plex Mono Regular and Symbols Nerd Font files; no upstream asset or source was changed. Installed font styles can supply bold/italic; the bundled regular face alone does not guarantee dedicated bold/italic faces. Program icons and their controls require the activity/icon unit; their presence in the draft settings is not evidence of completed icon parity.

The live theme reset defect is fixed in isolated core commit `f1ca146`; details and the precise upstream-update hotspot are in `qt-upstream-maintenance.md`. The regression failed before the fix, now passes, and preserves program OSC background overrides. Vendor code and lockfile remain unchanged. An initial settings acceptance run timed out under concurrent full Rune UI testing; the unchanged serial run passed. Run the large rendered suites separately to avoid contention. Updated Settings → Shortcuts navigation passed in `.build-tools/qt-keys-caxkf0ne/`.

Next: simplify and review the saved-host/settings cluster, then implement activity/program presentation, files and the remaining server/compatibility/native surfaces from the active plan. Full Linux source-build parity is still incomplete. Existing demo windows and personal sessions remain untouched. The checked-in private settings probe is reproducible with `python3 scripts/qt-preferences-private.py --driver qt/build/test-driver/rex-qt-test-driver --backend target/release/rex-qt-backend`.

The saved-host/settings simplification pass retains the proof tests unchanged. Scoped backend/host/keymap/preferences/workspace tests, both Qt builds and all-target/all-feature Clippy pass. Runtime settings pass again in `.build-tools/preferences-private-xg44hpl6/`; duplicate-label navigation and host isolation pass in `.build-tools/qt-hosts-c76y6fxs/`. These are private fixture artifacts, not published screenshots or environment data.

## Verified Activity and program presentation

The new Activity popup and program presentation read the public core's cached activity/status/progress, including unopened sessions; they do not attach background panes or add a polling engine. Focused Rust checks pass (25 backend and 13 activity-target tests, including shared host regressions), as do six native Activity model tests and scoped Clippy. Private `.build-tools/activity-private-hx5l9i3a/` proves unopened reports, live completion/ages, native filter/view/navigation and stable selection through a live wake. The actual 640×480 capture was inspected. Expanded `.build-tools/activity-private-qmr15svt/` passes seven native runs (145 events) and 24 surface scenarios: counts 0/1/2 and all four colour modes across tabs, pane headers, sidebar and Activity. Assertions inspect actual rendered label colours as well as logical selection/current-background roles; the 1024×700 multi-icon capture was visually inspected. Final primary replay also passes in `.build-tools/activity-private-ukyppveq/`. The original shell/editor/clipboard/resize/reconnect acceptance passes in `.build-tools/qt-acceptance-a6dael3j/`, and shortcut persistence/exact key bytes pass in `.build-tools/qt-keys-dohi5cyh/`.

Program glyphs use the existing licensed Symbols Nerd Font and the MIT frontend name-mapping policy. Superlogical's artwork is not bundled by Qt; its existing notices and files remain untouched. Generic program-family glyphs can differ from Rune's branded artwork. Future Rune frontend mapping additions need explicit downstream review even though connection/session/VT improvements remain shared.

One implementation worker invoked rustfmt without the task-local RUSTUP_HOME and added a 1.96.1 toolchain/update hash under the default Rust cache. No default settings file/change was observed. The addition is preserved; no unrelated files were removed. Shared parallel authoring was stopped under the workflow ownership rules, and partial path/upload/server work remains uncommitted and parked while activity is finished serially. Further integration proceeds one unit at a time.

An updated, independently owned local demo is running; ignored `.build-tools/current-parity-demo.json` records its launch metadata. Existing earlier demo and personal sessions were left untouched. Use the isolated launch command above for another fresh demo.

The user explicitly set the full-parity autonomous goal and requested more subagents. Three harness-managed isolated worktrees now continue paths/uploads, server controls/pairing, and native rendering/input in parallel. Each has independent copy-on-write build/Cargo/Zig/temp state and an explicit pinned-toolchain environment; no shared-source authoring resumed. Exact worker paths are kept locally in ignored `.build-tools/isolated-workers.json`. The original partial sources were checksum-archived under `.build-tools/parked-wave-source/` before removing only those owned copies from the primary checkout; current primary changes belong to activity integration. The delivery branch remains `feat/qt-quick-milestone`.

The canonical activity checks also pass all-target/all-feature Clippy, formatting, both Qt builds and focused backend/activity/host/keymap/workspace/preferences tests. Legacy runtime fixtures now complete on observed input or a post-assertion acknowledgement instead of short sleeps/quiet gaps: the exact cursor pixels, key edges and consumed-key assertions remain. This avoids losing transient proof under concurrent builds; performance verification remains a separate final gate.

## Verified paths and native file upload

Private paths/uploads are integrated into the primary branch after review of the isolated implementation. `.build-tools/qt-files-2kgir26l/result.json` passes native path paste, MIME clipboard/drop upload and API 10 refusal. The exact PTY capture contains a safely quoted `cd` insertion without Enter, followed by exactly two server-returned paths for two explicit MIME actions; uploaded bytes match the all-byte fixture. Recent paths persist in the authoritative Qt profile and remain endpoint-scoped. Backend tests (26), file tests (8), and all-target/all-feature Clippy pass.

Targets include host/epoch/session/window/pane; stale/superseded directory replies cannot become choices. Returned server client actions cannot open the person-only path browser: the known-action regression failed before the added shared registry guard and now passes. Public `ListDir`, upload `File` and `HostCommand::Upload` retain all protocol/transfer behavior. Upload status is requested-only; the public API provides neither in-flight cancellation nor completion/progress observation. Already-dispatched uploads keep their original target and are not replayed. Native MIME proof uses actual Qt events on an isolated offscreen window, not an automated gesture on the user's desktop.

Core queue-lifetime commit `c2fcb25` separately clears unconsumed callbacks/notices/bells at disconnect/reconnect. Actual WebSocket regressions failed before the fix; all 382 core tests now pass. No public API, vendor tree or lockfile changed.

## Server settings and controls checkpoint

Cached public core facts, explicit refresh, bounded page-local duration, sanitized listener copy and locally generated pairing codes are integrated. Stop and missing-SSH setup use captured single-use confirmations with Keep selected by default. Setup delegates to the public helper and requires explicit Connect or review/save afterward. Host, connection, report and backend-child changes invalidate pairing codes.

Primary verification: 26 backend, seven keymap and five server tests pass; locked release backend and both native Qt binaries build; all-target/all-feature Clippy and formatting pass. `.build-tools/qt-server-private-1y5ewroo/` verifies four native confirmation flows, stale/duplicate rejection and exactly one fake helper attempt. `.build-tools/qt-pair-native-19_i0q5u/` independently decodes the actual Qt Canvas QR and checks Escape/child-loss clearing. The 640×480 server page capture was inspected. Real tailnet pairing and real remote SSH setup remain unverified; their test boundaries are explicit. No core changes were needed.

## Native rendering and input checkpoint

Static graphics consume core-decoded pixels, including crop/placement/layer metadata. Device-pixel box/block shapes, underline variants, wide cursors, Linux physical keyboard identity and local IME preedit are implemented. No protocol decoder, connection logic or public-core change was added.

Primary evidence: 27 backend tests and 11 native QtTests pass (one offscreen native-seat case is explicitly skipped there and passes on its own private Wayland seat). Both Qt binaries and locked release backend build; all-target/all-feature Clippy and formatting pass. Actual PTY→Ghostty→Qt image/sprite/deletion/preedit/commit checks pass at 1× (`.build-tools/qt-render-didkjw3s/`) and 2× (`.build-tools/qt-render-1m7cc0oi/`), including exactly UTF-8 `é` and no preedit leakage. `.build-tools/native-layout-no8flh7w/` proves French logical A/native scan 24 maps to physical Q on a real private Wayland compositor. `.build-tools/qt-acceptance-gj0xknlh/` passes the full combined shell, Unicode/clipboard, Neovim save, resize, navigation and disconnect/reconnect/no-replay flow.

The first combined rendering fixture exceeded its setup/raw-input budget under concurrent build load. Its launcher now pastes fixture setup rather than synthesizing a long pathname character by character; exact keyboard behavior remains covered by the full acceptance suite. A bounded 30-second raw-input deadline preserves the exact-byte assertion. Native clipboard fixtures likewise separate setup from the keyboard/clipboard behavior under test. These timing changes do not relax image, clipboard or terminal-byte assertions. X11 native runtime and a third-party IME candidate engine remain unverified; terminal screen-reader semantics and full Kitty protocol expansion are outside this baseline.

Native clipboard replay on the combined build: `.build-tools/qt-wayland-t97hq3f8/` passes CLIPBOARD and PRIMARY ownership, an independent reader and exact middle-paste PTY bytes. Its setup/controller wait is bounded at 45 seconds and raw input at 60 seconds; the original exact payload assertions remain unchanged.

Activity presentation cleanup retains the same ordering, selection and icon policies while caching filtered rows, indexing backend joins, sharing the native icon-colour predicate and constructing server facts once per publication. Primary regressions remain unchanged. The scoped Rust tests, six Activity QtTests, both native builds, all-target/all-feature Clippy and private activity/files/server replays pass (`.build-tools/activity-private-5yh75u09/`, `qt-files-8oisbezm/`, `qt-server-private-pcopiixs/`).

## Desktop notification and sound checkpoint

Native asynchronous freedesktop delivery consumes every coherent publication once, replaces/withdraws per-tab notifications and preserves captured navigation. OSC notices use existing core queues and cached targets; missing layouts retain session-only navigation. Searchable native service health/test controls recover after failure. Sounds remain off by default and use a severity-coalesced bounded worker. Compositor focus is best effort through public Qt; private Wayland token APIs are intentionally not used.

Primary evidence: 30 backend tests pass, release backend and both Qt binaries build, all-target/all-feature Clippy and formatting pass. Five native service tests pass on a custom private bus with fake-only player paths, including rejection/recovery, daemon/bus replacement, exact click targets and sound timeout/reaping. `.build-tools/notification-window-scp51sx5/` passes actual OSC→Rust→Qt→fake-service delivery, native Settings rejection/recovery and minimum-size controls. Non-notification Qt fixtures now explicitly name absent private bus addresses to prevent desktop bus autolaunch.

Output benchmarking has identified a remaining real GUI starvation issue under sustained one/four-pane output (multi-second input latency); backend-only throughput is substantially faster. A measured event-loop fairness correction is in progress. This is a final parity blocker, not a passing performance claim.

## Downloads checkpoint

Native download entry, destination preference, large/free-space confirmation with Cancel focused, keyboard submission, all-host progress/cancel and folder-only opening are integrated through the public core. No transfer/store engine or core patch was added. Tests use actual private byte streams, a private low-space tmpfs namespace and fake desktop opening. Portable test destinations use owned temporary directories.

Primary verification: 30 backend, ten files, seven keymap and four preference tests pass; both Qt binaries and locked release backend build; formatting and all-target/all-feature Clippy pass. `.build-tools/qt-files-xq5pcj02/` passes all eight native reports: exact bytes, hostile-name sanitization, no overwrite, API refusal, default Cancel, keyboard confirmation/start, active cancellation, disconnect/exit partial cleanup and fake folder action. Remote download is currently inert with a timeout response; explicit default-deny replies are verified when the pending remote-control packet is integrated. No downloaded file was executed.

## Remote compatibility integration checkpoint

The exact eight-action authority, default-off saved-host consent, shared local view dispatcher, native authorization, typing/modal/field/sequence guards and one-shot deadline/epoch replies are integrated. More/host menus are included; a real private requester regression failed before this correction. Live-epoch opaque IDs remain retained for exact-once behavior; dead scopes are pruned and the memory tradeoff is documented in the protocol guide.

Primary scoped verification passes: 32 backend, 14 hosts, eight keymap, five remote tests, eight native action QtTests, both Qt builds, locked release backend, formatting and all-target/all-feature Clippy. `.build-tools/qt-remote-8d1c_qlf/` passes real private requests for all eight capabilities and explicit default-denial, including downloads. The isolated unit also passed 28 actual native steps and 28 requester calls. The first combined native replay reached More-menu refusal but exceeded its Settings controller deadline under the already measured GUI starvation issue (`.build-tools/qt-remote-ylxsonyw/`); repeat it after the pending state-projection correction before final parity sign-off.

Desktop work constraint: visible agent-operated demos belong on the dedicated `rex-qt-development` Hyprland workspace, never the user's primary workspace. The two older demo GUI processes were observed closed after the user reported a frozen popup/crowding; do not restart or reattach their sessions. Future visible launches must be routed to the dedicated workspace without changing the user's active workspace. Automated Qt checks stay offscreen or on their own headless compositor.

## Output responsiveness correction

A private one/four-pane benchmark attributed multi-second GUI stalls to repeated conversion of full terminal cells/images into QML bindings. QML now receives cached frame metadata only; C++ painting, raw snapshots, private wire data, notification consumption and event ordering are unchanged. An attempted event-loop fairness change did not improve the measurements and was not retained.

The same final fixture on the old production getter measured 2.6–8.7-second one-pane echoes and 1.4–3.8-second four-pane echoes. On the combined primary build, `.build-tools/qt-performance-ra6j43_n/` measures one-pane 205–300 ms and four-pane 190–200 ms during five seconds of 100 VT row updates/s per pane. Every pane completes output, input bytes are exact and actual server geometry is checked after real Qt ownership input. Timing has 100 ms driver polling resolution; this is software offscreen evidence, not physical scanout or maximum throughput. Snapshot traffic remains substantial (about 43 MB and 32 MB in these runs); no throughput promise is made.

Combined primary regressions pass: `.build-tools/qt-acceptance-6i8464rc/` full shell/editor/clipboard/resize/navigation/reconnect/no-replay; `qt-render-gs9gi2jx/` image/sprite/IME; `terminal-parity-bfsrraa3/` history/search and exact selection. Six terminal-fixture drag points now use the left half of odd-width cells, preserving the exact MOUSE/READY assertions. `.build-tools/qt-remote-y_m6y24b/` now passes the complete native consent/menu/typing authorization replay and explicit forbidden-action replies. An initial combined consent click was clipped; the strict replay passed unchanged, and public item-geometry diagnostics were retained. The earlier GUI starvation blocker is resolved for this measured workload.

GUI HOLD: The user requested all Rex Qt quit temporarily because the desktop was unusable. No Rex Qt GUI/backend processes remained at verification; two lingering Hyprland not-responding dialogs explicitly tied to Rex Qt were closed. Unrelated dialogs and all Rex servers were left alone. `.build-tools/GUI_PAUSED` records the hold. Continue only headless source/build/review work; do not launch any GUI or GUI test driver until explicit user resume, including on the dedicated workspace.

The user subsequently reported desktop recovery and requested app stability plus a durable workspace rule. `AGENTS.md` now requires isolated automated GUI probes, dedicated silent placement for visible computer-use/demo windows, and stopping task GUIs on desktop trouble. Resume offscreen/private-compositor verification only; visible demos remain closed until startup, popup, reconnect and output checks pass.

The latest user rule supersedes the earlier named-Hyprland-workspace suggestion: another window or workspace on the primary desktop is not isolation. `AGENTS.md` now includes Andrew's exact browser/desktop preferences. Use headless or a separate virtual display/agent desktop; no autonomous GUI/browser/terminal launch, focus change or workspace switch on the primary desktop. Visible human-demo launch instructions above are for an explicitly requested person-operated interaction, not an agent fallback.

## Maintenance integration checkpoint

Native Integrations status, fixed-command dry-run previews, explicit setup/removal, signed core release checks with source-build-only guidance, About notices and privacy-safe report copying are integrated. Existing local Rex CLI owns integration file rules; the public signed checker owns metadata validation. No installer, pacman helper, package replacement or additional protocol engine was introduced.

Primary evidence: 33 backend tests, public valid/tampered signature and signed loopback tests pass; both Qt binaries and locked release backend build; formatting and all-target/all-feature Clippy pass. `.build-tools/maintenance-private-ju8tqr8o/` passes native private-helper setup/removal/cancel/error, safe report copy and source-build update controls (11 fake helper calls). Its update-button probe intentionally fails at a private state lock before networking; signed success/tampering is independently verified at the public core loopback boundary. No real agent files, installed application or Internet release fetch was exercised.
