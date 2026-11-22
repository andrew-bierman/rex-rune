# Contributing to rex-tui

Use the [fork’s issue tracker](https://github.com/andrew-bierman/rex-tui/issues)
for rex-tui bugs, feature requests, Arch packaging, and the fork’s Qt frontend.
If you only see the problem in the upstream Rune frontend, use the
[Rex Rune issue tracker](https://github.com/shuv1337/rex-rune/issues).
If it happens in multiple clients, say which ones you tried.

## Report a bug

Search existing issues first. For a new report, include the smallest set of
steps that reproduces the problem, what you expected, and what happened.
Include the release or commit, your OS, terminal emulator, Rex server version
if known, and whether the server is local or remote. On Arch, `pacman -Q rex-tui`
prints the installed package version. If settings affect the bug, share the
relevant lines and omit credentials and private terminal output.

## Suggest a change

Describe the workflow you want to improve and where the current behavior gets
in your way. If you already have a patch, link the branch or commit in your
fork and include how you verified it. Maintainers review outside changes
through issues and credit authors when incorporating their work.

See the [TUI build and install guide](tui/README.md#install-on-arch-linux-and-omarchy)
and the [private demo capture](tui/README.md#capture-the-demo) for a reproducible Foot setup
with fictional workspace data. For Qt, see the
[verification driver](qt/README.md#verification-driver). Use disposable
sessions for checks that send input or change the workspace layout.
