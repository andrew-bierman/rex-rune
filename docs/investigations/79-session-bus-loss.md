# #79: optional desktop bus loss must not close terminals

## Scope and source

Base: `a8fc9e736620372a2f1e42045881abe44a9a5cd6` (`origin/master`).
Rune: vendored `d70929a4dae263d16d2e8088e7bfe53f58f619ad` plus documented patches.
No dependency bump, signal handler, notification-worker change or platform-wide loader shim.

Desktop preferences are best-effort reads, not terminal state. GSettings owns the
GNOME animation preference, Rune samples it at window creation/focus, and Rune's
separate zbus portal worker caches the colour preference. Rex owns terminal state
on the server; the client must not turn losing an optional bus into process death.
The notification worker has its own connection/recovery and health reporting.
There is no new durable operation, replay or recovery queue in this fix.

## Investigation notebook

Evidence root: `~/.cache/agent-ws/rex-issue79/`. Only private HOME/XDG, bus,
Xvfb/openbox and Rex sockets; no owner's services stopped. Times are Pacific.

1. Rebuilt the unmodified base with
   `CARGO_TARGET_DIR=/home/shuv/repos/rex-rune/target cargo build --locked --bin rex-rune`:
   exit 0 (`before-build.log`). Reused the existing shared build cache, not a shared
   source checkout.
2. Ran the original private reproducer from the #78 evidence with this binary:
   `REX_BIN=/home/shuv/repos/rex-rune/target/debug/rex-rune python3 ~/.cache/agent-ws/rex-m34-notify/parent-bus-sigterm.py "$PWD"`.
   Result: **one bus loss, one SIGTERM (-15)**. `before-native.log` and
   `rex-m34-notify/parent-sigterm-fshdbvnf/result.json`. No `rex-notifications`
   thread. Rules out the notification worker as a necessary cause.
3. GDB pending breakpoints on `g_bus_get_sync` and
   `g_dbus_connection_set_exit_on_close` identify the creator, not just nearby
   threads. `rex-m34-notify/parent-sigterm-og4t4bha/gdb.log` records:

   ```text
   g_bus_get_sync
   libgvfsdbus.so
   ... g_vfs_get_default
   g_file_new_for_path
   libdconfsettings.so
   ... g_settings_backend_get_default
   ... g_settings_new_full
   rune::platform::preferences::reduced_motion
   rune::window::Host::new (window.rs:541)
   ```

   The setter breakpoint records its second argument **1**, from the same GVfs
   connection construction. Thus Rune's GNOME reduced-motion read loads GSettings,
   dconf initializes GFile/GVfs, and GVfs acquires GIO's shared session connection
   with exit-on-close enabled. The truncated `rune-appearance` thread name is
   correlation, not the source of this GIO call. These loader-tracing runs stopped
   early under batch GDB and are evidence of construction only, not bus-loss runs.

4. A separate GDB run with only `catch signal SIGTERM` completes the causal
   chain: `rex-m34-notify/parent-sigterm-xo8yzv3b/gdb.log` shows `raise` inside
   GObject signal delivery from GIO's closed-connection handling on the dconf
   worker, then `Program terminated with signal SIGTERM`. Command:
   `REX_BIN=/home/shuv/repos/rex-rune/target/debug/rex-rune python3 ~/.cache/agent-ws/rex-issue79/trace-private.py "$PWD"`.
   The scratch wrapper runs GDB with `trace-signal.gdb`, while its private parent
   fixture terminates only its own bus. GDB's batch exit 0 is not the app's exit;
   the debugger's terminal-signal record is the evidence here.
5. Added the private native regression before changing Rune. It fails on the
   rebuilt base at its first bus loss: **SIGTERM -15**, exit 1
   (`regression-before.log`, `native-c9ikfue4/`). The independently repeated
   original reproducer and this regression give **2/2 baseline process exits**.
6. Fixed only the existing GSettings boundary: after validating schema/key and
   resolving GIO's public functions, acquire the shared session connection,
   clear exit-on-close, and hold the reference through backend construction.
   All references are released; missing bus/library/schema/symbols return the
   existing `None` preference outcome. No signal disposition is changed.
7. The separate zbus portal subscription now reports its end once to stderr:
   `Rune: desktop appearance updates unavailable; keeping the current appearance.`
   Its cached colour remains. This is a diagnostic, **not an in-window toast**,
   not notification-service success, and not a claim of automatic portal recovery.
8. Regression passes (`regression-after.log`, `native-bv5lywu0/`). Strengthened
   focus coverage to actually minimize/map/refocus the private window between
   checks; it passes again (`regression-focus-after.log`, `native-x6s6fdi0/`).
9. Planted a targeted violation, changing only `exit_on_close(bus, 0)` to `1`:
   rebuilt and ran the final fixture. It fails at the first bus loss with **-15**
   (`mutation-{build,test}.log`, `native-fi58634n/`). Restored `0`, rebuilt and
   reran: pass (`final-build.log`, `regression-final.log`, `native-825b58ur/`).
   This proves the test catches the connection policy, not merely the diagnostic.
10. Final fixture also pins `XDG_DATA_HOME` and clears inherited `XAUTHORITY`;
    rerun passes (`regression-final2.log`, `native-5qw_g60w/`).

## Verification recipe and results

All below ran on Linux on **2026-11-14 PST**, from this worktree, with
`CARGO_TARGET_DIR=/home/shuv/repos/rex-rune/target` for Cargo commands:

```sh
cargo build --locked --bin rex-rune
cargo fmt --all -- --check
cargo test --locked --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
REX_BIN=/home/shuv/repos/rex-rune/target/debug/rex-rune \
  python3 tests/private/session-bus-loss-x11.py
```

- Build, formatting, full tests and strict Clippy: **exit 0**. Full suite:
  **1,163 passed, 0 failed, 6 pre-existing ignores** (379 library, 776 app,
  8 integration). Logs and `.exit` receipts: `final-build`, `fmt`, `tests`,
  `clippy` under the evidence root. Rune's six pre-existing dependency warnings
  remain; no lint settings or hooks changed.
- Native fixture: **4/4 fixed runs passed**, each with two bus losses/restarts,
  native X11 keys producing independently asserted PTY files before, while
  absent, and after restart, plus a fresh process launched with no bus. **0/8
  process exits on bus loss**. The last three runs include real focus loss/gain.
  GIO/dconf/GVfs must be mapped or the fixture refuses to claim coverage.
  No notification worker starts. Each run records exactly **one** appearance
  diagnostic; SIGTERM still kills the app normally when explicitly sent at cleanup.
- Independently reran the existing opt-in notification-worker test, directly from
  the test executable built by Cargo, with private HOME/XDG, no display and fresh
  matching `REX_M34_PRIVATE_BUS` / `DBUS_SESSION_BUS_ADDRESS`:
  `rex_rune-d2bc2526e73a8a3e notification_worker_reconnects_after_a_private_bus_restart --ignored`.
  **1 passed**, no replay of the note lost with the bus (`notification-worker.log`,
  `worker-7vF7C6/bus-fixture/calls.jsonl`). This is separate from whole-app survival.
- `python3 -m py_compile tests/private/session-bus-loss-x11.py` and
  `git diff --check`: exit 0.

The fixture is in `tests/private/`, not `scripts/`: the latter path triggers an
unrelated existing Arch-helper workflow that runs a privileged/systemd container.
That is forbidden for this task. No CI definitions or gates changed; ordinary
Linux/macOS/Windows CI remains applicable to the PR. No container was run.

## Limits

- Actual acceptance is **X11 on this machine's GIO/dconf/GVfs**, not native
  Wayland, other GSettings modules/desktops, macOS or Windows. macOS/Windows code
  paths are unchanged; their CI results are for the lead to collect, not a claim
  of native checks here.
- The portal colour subscription remains stopped after bus loss and retains its
  last value; automatic appearance resubscription is not added. GNOME animation
  sampling is unavailable while the bus is absent and uses Rune's existing
  unavailable-preference fallback. Notification recovery retains its own owner.
- No real notification daemon, desktop audio, personal bus/display/Rex server,
  host package install/upgrade, personal client restart, merge, tag or release was used.

## Rat-stack guidance followed

Search/read used the provided `ratstack.sh` helper. Language-neutral guidance:

- `ratstack://repo/AGENTS.md`, `ratstack://skills/rat-stack-mode`: respect the
  product's own Rust/platform boundaries; do not import the TS/Effect stack.
- `ratstack://skills/investigate-a-failure`: reproduce, rebuild, prove the creator
  before changing policy, and retain the notebook and command receipts.
- `ratstack://lore/testing-ladder`, `ratstack://skills/test-behaviour`,
  `ratstack://lore/tests-that-earn-their-place`: a real native process/bus seam
  test catches SIGTERM; a mock or source-string assertion cannot. No new domain
  rule or lifecycle machine warrants generated properties/models here.
- `ratstack://lore/the-fence`: keep fmt/clippy/test gates and hooks intact.
- `ratstack://skills/durability`, `ratstack://lore/error-model`: isolate optional dependency failure, no replay;
  distinguish missing desktop service from service success or a program defect.
- Principle leaves read from `joelhooks/rat-stack` because helper leaf IDs return
  HTTP 422: `find-the-cause-first`, `rebuild-what-you-run`, `measure-real-limits`,
  `keep-execution-receipts`, `keep-unknown-distinct`,
  `tests-detect-broken-behavior` (under `skills/rat-stack-mode/principles/`), and
  `skills/durability/principles/fail-open-on-liveness.md`.

Helper HTTP 429/422 papercuts already exist as `pc_147c28c817cf` and
`pc_f7689c96f265`; no duplicate filed.
