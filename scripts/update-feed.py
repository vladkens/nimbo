# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
# /// script
# requires-python = ">=3.11"
# ///
"""Makes an update feed (src/update.rs): update.json and its Ed25519 signature update.json.sig.

Each package is `<platform> <url> <file>`: the platform key (`<os>-<arch>`, the os `macos`,
`windows` or `linux`, the arch `x86_64` or `aarch64`), where the file will be served, and the
file, whose size and SHA-512 go in the feed. Signs with openssl 3
and the private key from `openssl genpkey -algorithm ed25519 -out update.pem`.

uv run scripts/update-feed.py <key.pem> <version> [--notes <text>] [--out <dir>] <package>...
"""

import argparse
import base64
import hashlib
import json
import subprocess
from pathlib import Path

PLATFORMS = {f"{os}-{arch}" for os in ("macos", "windows", "linux") for arch in ("x86_64", "aarch64")}


def main() -> None:
  ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
  ap.add_argument("key", type=Path, help="the Ed25519 private key, PEM")
  ap.add_argument("version")
  ap.add_argument("package", nargs="+", help="<platform> <url> <file>, repeated")
  ap.add_argument("--notes", default="")
  ap.add_argument("--out", type=Path, default=Path("."))
  args = ap.parse_args()
  if len(args.package) % 3:
    ap.error("each package is <platform> <url> <file>")

  def file(url: str, path: str) -> dict:
    data = Path(path).read_bytes()
    return {"url": url, "size": len(data), "sha512": hashlib.sha512(data).hexdigest()}

  it = iter(args.package)
  packages = list(zip(it, it, it))
  if bad := sorted({p for p, _, _ in packages} - PLATFORMS):
    ap.error(f"unknown platform {', '.join(bad)}: one of {', '.join(sorted(PLATFORMS))}")

  platforms = {p: file(u, f) for p, u, f in packages}
  pld = {"version": args.version, "notes": args.notes, "platforms": platforms}
  feed = args.out / "update.json"
  feed.write_text(json.dumps(pld, indent=2) + "\n")

  cmd = ["openssl", "pkeyutl", "-sign", "-inkey", str(args.key), "-rawin", "-in", str(feed)]
  sig = subprocess.run(cmd, check=True, capture_output=True).stdout
  (args.out / "update.json.sig").write_text(base64.b64encode(sig).decode() + "\n")
  print(f"{feed} and its .sig: {args.version} for {', '.join(platforms)}")


if __name__ == "__main__":
  main()
