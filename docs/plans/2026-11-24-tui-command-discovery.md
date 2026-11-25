# rex-tui command discovery

- Make the F1 shortcuts view searchable and executable. Keep existing bindings and the Settings tab. Use the existing action dispatch so confirmations and focus restrictions still apply.
- Delegate Arch launcher arguments to the existing Rust CLI. Check help, version, and settings through the packaged layout.
- Update the roadmap to distinguish merged capabilities from remaining work. Record the installed Rex server API and the client's negotiated range before attempting Lua prompt support. Keep Qt behavior and shared protocol unchanged in this PR.
- Verify the command search with unit tests and exercise the menu in an isolated PTY.

## Rex Lua prompt compatibility

A disposable local Rex 0.1.0-preview.22 server accepted WebSocket control
hellos pinned to revision 13 and separately to revision 17. The shipped client
still advertises revisions 10–13. A follow-up can add compatible revision 17
negotiation and handle prompt requested and ended events, plus client replies.
Verify submit, cancel, and dismissal after a second client answers before
advertising support. [Rex Custom UI](https://www.superlogical.com/rex/docs/customize/custom-ui)
