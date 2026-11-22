#!/usr/bin/env python3
"""Build and stage the two locked rex-tui executables for a single Rust target."""

import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def host_target():
    result = subprocess.check_output(
        [os.environ.get("RUSTC", "rustc"), "-vV"], cwd=ROOT, text=True
    )
    return next(line.removeprefix("host: ") for line in result.splitlines()
                if line.startswith("host: "))


def build_pair(target, target_dir):
    cargo = os.environ.get("CARGO", "cargo")
    command = [
        cargo, "build", "--locked", "--release", "--target", target, "--target-dir",
        str(target_dir), "--manifest-path",
    ]
    for manifest, options in (
        (ROOT / "Cargo.toml", ["--no-default-features", "--bin", "rex-qt-backend"]),
        (ROOT / "tui" / "Cargo.toml", ["--bin", "rex-tui"]),
    ):
        subprocess.run([*command, str(manifest), *options], cwd=ROOT, check=True)
    suffix = ".exe" if "-windows-" in target else ""
    release = target_dir / target / "release"
    return [release / f"{binary}{suffix}" for binary in ("rex-tui", "rex-qt-backend")]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="New directory for the pair")
    parser.add_argument("--target", help="Rust target triple (defaults to the host target)")
    parser.add_argument("--target-dir", type=Path, default=ROOT / "target" / "tui-portable")
    args = parser.parse_args()
    target = args.target or host_target()
    if not re.fullmatch(r"[a-z0-9_]+(?:-[a-z0-9_]+)+", target):
        parser.error("expected a Rust target triple")
    output = args.output.resolve()
    if output.exists():
        parser.error(f"output already exists: {output}")
    binaries = build_pair(target, args.target_dir.resolve())
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".rex-tui-", dir=output.parent) as scratch:
        staged = Path(scratch) / "pair"
        staged.mkdir()
        for source in [*binaries, ROOT / "LICENSE"]:
            shutil.copy2(source, staged / source.name)
        staged.rename(output)
    print(output)


if __name__ == "__main__":
    main()
