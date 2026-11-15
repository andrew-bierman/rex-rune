# Keeping Qt downstream of Rex Rune

The user settled this architecture on 2026-11-15: consume Rex Rune improvements through an upstream-tracking fork, keeping Qt additive and optional. `origin` is `https://github.com/andrew-bierman/rex-rune.git`; `upstream` is `https://github.com/shuv1337/rex-rune.git`. Never publish branches or PRs against upstream, and never merge the delivery PR automatically.

## Boundary and diff audit

The only upstream build integration is the four-line `rex-qt-backend` binary entry in `Cargo.toml`. Rust adapter code is private to that executable in `qt/backend/`; C++/QML is in `qt/`. The only core source change is the isolated live-theme reset fix below. `vendor/` and `Cargo.lock` remain unchanged. Original default `ui` features and Rune targets are unchanged. MIT licensing and upstream/vendor attribution remain intact; no upstream code was copied into a second implementation.

The adapter translates JSON requests into existing `HostCommand`/terminal commands and serializes existing `HostModel`/Ghostty `Frame` state. Its pending navigation IDs, selected pane and stale-target guards are frontend interaction state, not a replacement session/reconnection engine. Qt's frame rendering, keyboard event conversion and clipboard are necessarily frontend-specific. Socket connection, reconnect/backoff, session discovery, stream ownership, key delivery, resize delivery, authoritative ownership state and VT parsing stay in upstream workers.

A separate frontend crate was evaluated against the actual diff. It would require a workspace or independent lockfile and path-dependency patch handling while removing no additional upstream source edit. The existing standalone binary already excludes Rune UI with `--no-default-features`; retain it until crate separation provides a concrete maintenance benefit. Do not add a general frontend framework for hypothetical GTK reuse.

Future unavoidable core changes must each have a separate narrow commit explaining the public API or upstream defect, with tests and an entry here. The first milestone needed none; full parity runtime verification exposed the narrowly scoped live-theme defect below. Earlier commits added an adapter library export and a newer-Clippy Rune-test cleanup; the downstream-isolation commit removes both. Do not cherry-pick those intermediate commits without their removal.

## Update procedure

Prefer merge on already-published downstream branches, avoiding force pushes. Rebase is reasonable only for unpublished work. Start clean; inspect incoming changes before executing the candidate checks. This creates a local candidate and never updates the delivery branch implicitly:

```sh
git status --short
git remote -v
git fetch upstream master
git log --oneline HEAD..upstream/master
git worktree add -b maintenance/upstream-candidate ../rex-rune-qt-upstream-candidate HEAD
cd ../rex-rune-qt-upstream-candidate
git merge --no-edit upstream/master
# Resolve conflicts here; retain upstream core behavior and adapt Qt to it.
cargo build --locked --release --bin rex-rune
cargo test --locked --all-features
cargo test --locked --no-default-features --bin rex-qt-backend
cargo build --locked --release --no-default-features --bin rex-qt-backend
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --manifest-path vendor/rexrs/Cargo.toml --all-features
mkdir -p qt/build
cd qt/build
qmake6 ../rex-qt.pro
make -j2
mkdir -p test-driver
cd test-driver
qmake6 ../../rex-qt.pro CONFIG+=test_driver
make -j2
cd ../../..
python3 scripts/qt-private.py --native-wayland-smoke
```

Use the repository-required Rust/Zig versions and installed Qt requirements from `qt/README.md`. Qt acceptance needs Rex, Neovim and an owned disposable server; never substitute the personal endpoint. The native smoke needs an available Wayland compositor. Without one, run the offscreen checks and record native verification as missing. Task-local toolchains/caches are not tracked and must be configured explicitly in a new worktree. After checking logs and reviewing the candidate diff, publish only to an owned-fork branch/PR; do not merge automatically. Remove only this disposable worktree when finished.

## Tested rehearsal and practical limits

On 2026-11-15, `git fetch upstream master` resolved to `8c1382a0ecfe542c57ffc3608efb6762c4877be2`. The local integration rehearsal uses a detached disposable worktree and `git merge --no-edit upstream/master`; upstream is already an ancestor, so the expected result is `Already up to date.` This tests the actual fetch/worktree/merge path, not a hypothetical conflict resolution. No new upstream commits were available, so compatibility with a future release is not established by this rehearsal. Checks of the resulting source are recorded in `qt-quick-status.md`.

Likely hotspots: the additive Cargo binary list; public `HostCommand`, `HostModel`, terminal sender and Ghostty frame fields consumed in `qt/backend/`; renamed core APIs; terminal ownership/key semantics; upstream vendored versions and their toolchain requirements. Preserve upstream lockfile/vendor updates rather than pinning stale dependencies to hide incompatibilities. README and `.gitignore` additions may have trivial textual conflicts. Public core APIs have no stable downstream compatibility promise.

Rust 1.100 adds a Clippy warning in an unchanged upstream activity UI test. Prefer upstream's supported toolchain when enforcing its gate; do not edit unrelated Rune tests merely for local toolchain drift. GitHub Actions on the owned fork currently has zero registered workflows/runs despite source definitions, so remote CI is unavailable evidence; registration is optional and does not block local development or use.

## Isolated core patch: live theme defaults

The commit titled `fix(terminal): refresh omitted live theme defaults` changes only terminal theme metadata application and its regression test in `src/terminal.rs`, plus this maintenance record. Ghostty's render update assigns foreground/background together only when both effective colours exist. Passing `None` for omitted theme values left the previous rendered pair cached: switching from Rosé Pine to Follow server kept the old live background, while a fresh attachment correctly displayed black.

The patch supplies Ghostty's built-in white foreground and black background for omitted defaults in a theme event. Explicit server colours and program OSC overrides retain precedence. It adds no Qt dependency, transport change, or local frontend recolouring. The regression exercises sparse theme application, live clearing without program output, and preservation of a program background override. Remove this workaround when the pinned Ghostty renderer handles absent defaults equivalently; re-run the terminal regression and the private Qt preferences acceptance before doing so. This small metadata handler is now an explicit upstream-update conflict hotspot; keep the fix separate from Qt frontend commits.
