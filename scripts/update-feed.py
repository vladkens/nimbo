# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
# /// script
# requires-python = ">=3.11"
# ///
"""Makes an update feed (src/update.rs): update.json and its Ed25519 signature update.json.sig.

Each package is `<platform> <url> <file>`: the platform key (`macos-aarch64`, `windows-x86_64`,
`linux-x86_64`), where the file will be served, and the file, hashed here. Signs with openssl 3
and the private key from `openssl genpkey -algorithm ed25519 -out update.pem`.

uv run scripts/update-feed.py <key.pem> <version> [--notes <text>] [--out <dir>] <package>...
"""

import argparse
import base64
import hashlib
import json
import subprocess
from pathlib import Path


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

  def sha512(path: str) -> str:
    return hashlib.sha512(Path(path).read_bytes()).hexdigest()

  it = iter(args.package)
  platforms = {p: {"url": u, "sha512": sha512(f)} for p, u, f in zip(it, it, it)}
  pld = {"version": args.version, "notes": args.notes, "platforms": platforms}
  feed = args.out / "update.json"
  feed.write_text(json.dumps(pld, indent=2) + "\n")

  cmd = ["openssl", "pkeyutl", "-sign", "-inkey", str(args.key), "-rawin", "-in", str(feed)]
  sig = subprocess.run(cmd, check=True, capture_output=True).stdout
  (args.out / "update.json.sig").write_text(base64.b64encode(sig).decode() + "\n")
  print(f"{feed} and its .sig: {args.version} for {', '.join(platforms)}")


if __name__ == "__main__":
  main()
