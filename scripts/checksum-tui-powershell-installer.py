#!/usr/bin/env python3
"""Embed verified Windows archive hashes in cargo-dist 0.34's PowerShell installer."""

import argparse
import hashlib
import json
from pathlib import Path
import tempfile

TARGETS = ("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")
CHECK_TARGET = '  $artifact_name = $info["artifact_name"]\n'
CHECK_DOWNLOAD = '  Invoke-DownloadFile -client $wc -url $url -path $dir_path\n'


def patch(installer, manifest):
    if manifest["dist_version"] != "0.34.0":
        raise ValueError("Review the new cargo-dist PowerShell template before patching it")
    source = installer.read_text(encoding="utf-8")
    if "$rex_tui_checksums" in source or any(
        source.count(anchor) != 1 for anchor in (CHECK_TARGET, CHECK_DOWNLOAD)
    ):
        raise ValueError("Expected an unpatched cargo-dist PowerShell installer")

    checksums = {}
    for target in TARGETS:
        filename = f"rex-tui-{target}.zip"
        artifact = manifest["artifacts"].get(filename, {})
        expected = artifact.get("checksums", {}).get("sha256")
        if not expected:
            continue
        if artifact["target_triples"] != [target]:
            raise ValueError(f"Unexpected platform metadata for {filename}")
        with installer.with_name(filename).open("rb") as archive:
            digest = hashlib.file_digest(archive, "sha256").hexdigest()
        if digest != expected:
            raise ValueError(f"Manifest SHA-256 mismatch for {filename}")
        checksums[filename] = digest
    if not checksums:
        raise ValueError("A verified Windows archive is required for a PowerShell installer")
    entries = "\n".join(f'    "{name}" = "{digest}"' for name, digest in checksums.items())
    target_check = f'''
  $rex_tui_checksums = @{{
{entries}
  }}
  if (-not $rex_tui_checksums.ContainsKey($artifact_name)) {{
    throw "No verified rex-tui archive for $arch ($artifact_name)"
  }}
'''
    download_check = '''
  $actual = (Get-FileHash -LiteralPath $dir_path -Algorithm SHA256 -ErrorAction Stop).Hash
  if ($actual -ne $rex_tui_checksums[$artifact_name]) {
    throw "SHA-256 checksum mismatch for $artifact_name"
  }
'''
    patched = source.replace(CHECK_TARGET, CHECK_TARGET + target_check)
    patched = patched.replace(CHECK_DOWNLOAD, CHECK_DOWNLOAD + download_check)
    with tempfile.TemporaryDirectory(dir=installer.parent) as temp:
        output = Path(temp) / installer.name
        output.write_text(patched, encoding="utf-8")
        output.replace(installer)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("installer", type=Path)
    parser.add_argument("manifest", type=Path)
    args = parser.parse_args()
    patch(args.installer, json.loads(args.manifest.read_text(encoding="utf-8")))


if __name__ == "__main__":
    main()
