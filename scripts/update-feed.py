# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
# /// script
# requires-python = ">=3.11"
# ///
"""Makes an update feed (src/update.rs): update.txt and its Ed25519 signature update.txt.sig.

Each package is `<platform> <url> <file>`: the platform key (`<os>-<arch>`, the os `macos`,
`windows` or `linux`, the arch `x86_64` or `aarch64`), where the file will be served, and the
file, whose size and SHA-512 go in the feed. Signs with openssl 3 and the private key from
`openssl genpkey -algorithm ed25519 -out update.pem`; `--public-key` prints its public half in
hex, the app's `Feed::key`.

uv run scripts/update-feed.py <key.pem> <version> [--out <dir>] <package>...
uv run scripts/update-feed.py <key.pem> --public-key
"""

import argparse
import hashlib
import subprocess
from pathlib import Path

PLATFORMS = {f"{os}-{arch}" for os in ("macos", "windows", "linux") for arch in ("x86_64", "aarch64")}


def openssl(*args: str) -> bytes:
  return subprocess.run(["openssl", *args], check=True, capture_output=True).stdout


def main() -> None:
  ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
  ap.add_argument("key", type=Path, help="the Ed25519 private key, PEM")
  ap.add_argument("version", nargs="?")
  ap.add_argument("package", nargs="*", help="<platform> <url> <file>, repeated")
  ap.add_argument("--out", type=Path, default=Path("."))
  ap.add_argument("--public-key", action="store_true", help="print the public key in hex")
  args = ap.parse_args()
  if args.public_key:
    # The DER of an Ed25519 public key ends in its 32 bytes.
    print(openssl("pkey", "-in", str(args.key), "-pubout", "-outform", "DER")[-32:].hex())
    return

  if not args.version or not args.package or len(args.package) % 3:
    ap.error("give a version and packages, each <platform> <url> <file>")

  it = iter(args.package)
  packages = list(zip(it, it, it))
  if bad := sorted({p for p, _, _ in packages} - PLATFORMS):
    ap.error(f"unknown platform {', '.join(bad)}: one of {', '.join(sorted(PLATFORMS))}")

  if len({p for p, _, _ in packages}) < len(packages):
    ap.error("a platform is given twice")

  if any(c.isspace() for c in args.version + "".join(u for _, u, _ in packages)):
    ap.error("the version and the urls hold no whitespace")

  lines = [f"version {args.version}"]
  for platform, url, path in packages:
    data = Path(path).read_bytes()
    lines.append(f"{platform} {len(data)} {hashlib.sha512(data).hexdigest()} {url}")

  args.out.mkdir(parents=True, exist_ok=True)
  feed = args.out / "update.txt"
  feed.write_bytes("".join(f"{line}\n" for line in lines).encode())

  sig = openssl("pkeyutl", "-sign", "-inkey", str(args.key), "-rawin", "-in", str(feed))
  (args.out / "update.txt.sig").write_bytes(sig.hex().encode() + b"\n")
  print(f"{feed} and its .sig: {args.version} for {', '.join(p for p, _, _ in packages)}")


if __name__ == "__main__":
  main()
