# Rex Rune patch

This is the crates.io `ghostty-vt-sys` 0.8.0+ghostty.1.4.0-main.83fb956
package. The original licenses and upstream source lock are preserved.

Rex needs the exact OSC override state for the first 16 palette entries.
The build wrapper adds a read-only terminal getter to its own verified
Ghostty build copy, before invoking Zig. The fixed source URL, hash, Zig
dependency hashes, and existing C ABI identifiers remain the same.
Cargo, Windows CI, and Nix use the same patch path. The optional
`pkg-config` path requires headers advertising the added getter before
using an installed library.

Remove the native patch and wrapper when the upstream C API exposes the
override mask.
