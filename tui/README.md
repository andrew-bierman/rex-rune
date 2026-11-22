# Rex TUI prototype

A Ratatui frontend for Rex with a persistent host and session sidebar, a tab
strip, and live terminal panes. It uses the fork's existing backend for Rex
connections and opens sessions that already exist on the server.

Build and run from the repository root:

```sh
mise exec -- cargo build --locked --release --no-default-features --bin rex-qt-backend
mise exec -- cargo build --locked --release --manifest-path tui/Cargo.toml
mkdir -p "${XDG_CONFIG_HOME:-$HOME/.config}/rex-tui"
jq '{hosts: [.hosts[] | {label, endpoint}]}' \
  "${XDG_CONFIG_HOME:-$HOME/.config}/rex-qt/workspace.json" \
  > "${XDG_CONFIG_HOME:-$HOME/.config}/rex-tui/workspace.json"
./tui/target/release/rex-tui ./target/release/rex-qt-backend \
  "${XDG_CONFIG_HOME:-$HOME/.config}/rex-tui/workspace.json"
```

The example copies the host catalog into a separate profile. Run the copy only
when setting up that profile. In the sidebar, use arrows or j/k to move and
Enter to open. F6 switches input between navigation and the terminal. F3/F4
switch tabs, F7 cycles panes, and F10 exits without ending server sessions.

This prototype renders text snapshots and existing splits. It does not yet
create sessions or splits, forward mouse events, display inline graphics, or
offer scrollback search. The backend currently transmits full JSON snapshots.
