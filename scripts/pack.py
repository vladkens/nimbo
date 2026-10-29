# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
"""Makes an app's release files from its Cargo.toml: the macOS app and DMG, the Windows
installer, the AppImage and .deb, and the signed update feed (src/update.rs).

The app describes itself in `[package.metadata.nimbo]` (`id`, `name`, `icon`, `comment`, and the
tables `macos` and `linux`); the version and the binary's name are the package's. All of it comes
from `cargo metadata --no-deps`, which compiles nothing, so this runs on a plain `python3` (3.9,
macOS's own) with the standard library. The binary is already built, by default
`<target dir>/<triple>/dist/<bin>`; the target defaults to this machine's arch on the command's
OS. Each OS's files are made by its module: pack_macos.py, pack_windows.py, pack_linux.py. Names:
`<Name>-v<version>-<platform>.<ext>`, the .deb as Debian names it.

A job that signs compiles nothing: the macOS one (vladkens/actions' macos-sign) finds nimbo at
the rev the app's Cargo.lock pins (`source = "git+https://github.com/vladkens/nimbo…#<sha>"`),
checks it out and runs this `pack.py dmg` from there.

`feed` writes update.txt: `version <v>`, a line per file, `<platform> <size> <sha512> <url>`, and
last `sig <hex>`, the Ed25519 signature of every byte before it. A file's platform comes from its
name (`linux-aarch64`, `windows-x64`) or is given, `<platform>=<file>`; its url is `--url` (where
`{version}` is the version) and the file's name. The private key, from `--key <file>` or
`$UPDATE_KEY`, is its 32-byte seed in hex (as `feed-key` makes it) or a PEM; openssl 3 signs.
`feed-key` makes the one key, in memory only: it sets the repo's secret UPDATE_KEY (gh), puts the
private key on the clipboard to keep (Bitwarden) and prints the public key in hex, the app's
`Feed::key`. `feed --public-key` prints that of a key it is given.

python3 scripts/pack.py app|setup|appimage|deb [--target <triple>] [--bin <path>] [--out <dir>] [--manifest <Cargo.toml>]
python3 scripts/pack.py dmg [<Name>.app] [--out <dir>] [--manifest <Cargo.toml>]
python3 scripts/pack.py feed [--key <file>] --url <base> [--out <dir>] [<platform>=]<file>...
python3 scripts/pack.py feed [--key <file>] --public-key
python3 scripts/pack.py feed-key [--repo <owner/name>]
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import platform as host
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib
from dataclasses import dataclass
from pathlib import Path

PLATFORMS = {f"{o}-{a}" for o in ("macos", "windows", "linux") for a in ("x86_64", "aarch64")}
# The triple's OS of each command, for the default target.
OS = {
  "app": "apple-darwin",
  "dmg": "apple-darwin",
  "setup": "pc-windows-msvc",
  "appimage": "unknown-linux-gnu",
  "deb": "unknown-linux-gnu",
}
# An Ed25519 private key's PKCS#8 DER (RFC 8410) before its 32-byte seed.
SEED_DER = bytes.fromhex("302e020100300506032b657004220420")
PNG = b"\x89PNG\r\n\x1a\n"


@dataclass
class Config:
  id: str
  name: str
  icon: Path
  comment: str
  version: str
  bin: str
  macos: dict
  linux: dict
  windows: dict
  publisher: str
  authors: list
  root: Path
  target: Path


def run(*args, **kw) -> subprocess.CompletedProcess:
  return subprocess.run([str(a) for a in args], check=True, **kw)


def metadata(manifest: Path) -> dict:
  if not shutil.which("cargo"):
    sys.exit("pack: cargo not found: `cargo metadata` reads the config")

  args = ["--format-version", "1", "--no-deps", "--manifest-path", manifest]
  out = run("cargo", "metadata", *args, capture_output=True)
  return json.loads(out.stdout)


def parse(meta: dict, manifest: Path) -> Config:
  """The config of the package at `manifest` in `cargo metadata`'s output, its paths under the
  manifest's folder.

  >>> pkg = {"name": "a", "version": "1.2.0", "manifest_path": "/x/Cargo.toml", "default_run": None}
  >>> pkg["targets"] = [{"name": "a", "kind": ["bin"]}]
  >>> pkg["metadata"] = {"nimbo": {"id": "me.a", "name": "A", "icon": "i.png"}}
  >>> c = parse({"packages": [pkg]}, Path("/x/Cargo.toml"))
  >>> c.bin, c.version, c.icon.name, c.macos
  ('a', '1.2.0', 'i.png', {})
  """
  manifest = manifest.resolve()
  pkg = next((p for p in meta["packages"] if Path(p["manifest_path"]).resolve() == manifest), None)
  if pkg is None:
    sys.exit(f"pack: {manifest} is no package")

  nimbo = (pkg.get("metadata") or {}).get("nimbo")
  if nimbo is None:
    sys.exit(f"pack: {manifest} has no [package.metadata.nimbo]")

  missing = [k for k in ("id", "name", "icon") if not isinstance(nimbo.get(k), str)]
  if missing:
    sys.exit(f"pack: [package.metadata.nimbo] needs {', '.join(missing)}")

  # The one binary, else the one cargo runs, else the package's own.
  bins = [t["name"] for t in pkg["targets"] if "bin" in t["kind"]]
  bin = bins[0] if len(bins) == 1 else pkg.get("default_run") or pkg["name"]
  authors = pkg.get("authors") or []
  return Config(
    id=nimbo["id"],
    name=nimbo["name"],
    icon=manifest.parent / nimbo["icon"],
    comment=nimbo.get("comment", ""),
    version=pkg["version"],
    bin=bin,
    macos=nimbo.get("macos", {}),
    linux=nimbo.get("linux", {}),
    windows=nimbo.get("windows", {}),
    # `[package.metadata.nimbo] publisher`, else the first author without the mail.
    publisher=nimbo.get("publisher") or next(iter(authors), "").split(" <")[0],
    authors=authors,
    root=manifest.parent,
    target=Path(meta.get("target_directory") or manifest.parent / "target"),
  )


def load(manifest: Path) -> Config:
  return parse(metadata(manifest), manifest)


def arch(target: str) -> str:
  """The arch of a target triple, as Rust names it.

  >>> arch("aarch64-apple-darwin"), arch("x86_64-pc-windows-msvc"), arch("arm64")
  ('aarch64', 'x86_64', 'aarch64')
  """
  name = target.split("-")[0].lower()
  name = {"arm64": "aarch64", "amd64": "x86_64"}.get(name, name)
  if name not in ("x86_64", "aarch64"):
    sys.exit(f"pack: {target}: the arch is x86_64 or aarch64")

  return name


def platform(name: str) -> str | None:
  """A file's feed platform by its name, None when the name doesn't say.

  >>> platform("Wavo-v1.2.0-linux-aarch64.AppImage"), platform("Wavo-v1.2.0-windows-x64-setup.exe")
  ('linux-aarch64', 'windows-x86_64')
  >>> platform("Wavo-v1.2.0-macos.zip") is None
  True
  """
  m = re.search(r"-(macos|windows|linux)-(x86_64|aarch64|x64|arm64)(?=[-.]|$)", name)
  return f"{m[1]}-{arch(m[2].replace('x64', 'x86_64'))}" if m else None


def body(version: str, packages: list[tuple[str, str, bytes]]) -> bytes:
  """The feed before its signature, of (platform, url, file's bytes).

  >>> body("1.2", [("linux-x86_64", "https://a/b", b"abc")]).decode().split()[:4]
  ['version', '1.2', 'linux-x86_64', '3']
  """
  lines = [f"version {version}"]
  for p, url, data in packages:
    lines.append(f"{p} {len(data)} {hashlib.sha512(data).hexdigest()} {url}")

  return "".join(f"{line}\n" for line in lines).encode()


def openssl(*args, input: bytes | None = None) -> bytes:
  return run("openssl", *args, input=input, capture_output=True).stdout


def pem(key: str) -> bytes:
  """The private key as openssl reads it: a PEM as it is, a hex seed wrapped in PKCS#8.

  >>> pem("00" * 32).splitlines()[0]
  b'-----BEGIN PRIVATE KEY-----'
  """
  key = key.strip()
  if not re.fullmatch(r"[0-9a-fA-F]{64}", key):
    return key.encode() + b"\n"

  b64 = base64.b64encode(SEED_DER + bytes.fromhex(key)).decode()
  return f"-----BEGIN PRIVATE KEY-----\n{b64}\n-----END PRIVATE KEY-----\n".encode()


def public_key(key: bytes) -> str:
  # The DER of an Ed25519 public key ends in its 32 bytes.
  return openssl("pkey", "-pubout", "-outform", "DER", input=key)[-32:].hex()


def sign(key: bytes, file: Path) -> bytes:
  # openssl signs Ed25519 only from files: the key's is 0600 (mkstemp) and gone at once.
  fd, path = tempfile.mkstemp(suffix=".pem")
  try:
    with os.fdopen(fd, "wb") as f:
      f.write(key)

    return openssl("pkeyutl", "-sign", "-inkey", path, "-rawin", "-in", file)
  finally:
    os.unlink(path)


def feed(key: bytes, version: str, url: str, files: list[str], out: Path) -> Path:
  base = url.replace("{version}", version).rstrip("/")
  packages = []
  for arg in files:
    p, _, path = arg.partition("=")
    if p not in PLATFORMS:
      p, path = platform(Path(arg).name), arg

    if p is None:
      sys.exit(f"pack: {arg}: its name doesn't say its platform, give it: <platform>=<file>")

    file = Path(path)
    packages.append((p, f"{base}/{file.name}", file.read_bytes()))

  if len({p for p, _, _ in packages}) < len(packages):
    sys.exit("pack: a platform is given twice")

  if any(c.isspace() for c in version + "".join(u for _, u, _ in packages)):
    sys.exit("pack: the version and the urls hold no whitespace")

  out.mkdir(parents=True, exist_ok=True)
  path = out / "update.txt"
  path.write_bytes(body(version, packages))
  sig = sign(key, path)
  with path.open("ab") as f:
    f.write(f"sig {sig.hex()}\n".encode())

  print(f"{path}: {version} for {', '.join(p for p, _, _ in packages)}, signed")
  return path


def clipboard() -> list[str]:
  wayland = os.environ.get("WAYLAND_DISPLAY")
  tools = (
    ["pbcopy"],
    ["clip"],
    *([["wl-copy"]] if wayland else []),
    ["xclip", "-selection", "clipboard"],
  )
  found = next((t for t in tools if shutil.which(t[0])), None)
  if found is None:
    sys.exit(
      "pack: no clipboard (pbcopy, clip, wl-copy or xclip) to hand the private key over: no key made"
    )

  return found


def feed_key(repo: str | None) -> None:
  copy = clipboard()
  key = openssl("genpkey", "-algorithm", "ed25519")
  seed = openssl("pkey", "-outform", "DER", input=key)[-32:].hex()
  run("gh", "secret", "set", "UPDATE_KEY", *(["--repo", repo] if repo else []), input=seed.encode())
  run(*copy, input=seed.encode())
  print(
    "pack: the private key is set as UPDATE_KEY and on the clipboard: keep it in Bitwarden",
    file=sys.stderr,
  )
  print(public_key(pem(seed)))


# MARK: PNG


def chunks(png: bytes):
  """The PNG's chunks, type and data."""
  at = 8
  while at < len(png):
    (n,) = struct.unpack(">I", png[at : at + 4])
    yield png[at + 4 : at + 8], png[at + 8 : at + 8 + n]
    at += 12 + n


def decode(png: bytes) -> tuple[int, int, bytearray] | None:
  """Width, height and RGBA bytes of an 8-bit (or paletted) non-interlaced PNG; `None` else.

  >>> decode(encode(2, bytes([255, 0, 0, 255, 0, 0, 0, 0] * 2)))[2][:8]
  bytearray(b'\\xff\\x00\\x00\\xff\\x00\\x00\\x00\\x00')
  """
  if png[:8] != PNG:
    return None

  head, data, palette, trns = b"", [], b"", b""
  for kind, body in chunks(png):
    if kind == b"IHDR":
      head = body
    elif kind == b"IDAT":
      data.append(body)
    elif kind == b"PLTE":
      palette = body
    elif kind == b"tRNS":
      trns = body

  w, h, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", head)
  channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}.get(color)
  if not channels or interlace or not (depth == 8 or (color == 3 and depth in (1, 2, 4))):
    return None

  bpp = max(1, channels * depth // 8)
  stride = (w * channels * depth + 7) // 8
  raw = unfiltered(zlib.decompress(b"".join(data)), stride, h, bpp)
  if color == 3:
    alphas = trns + b"\xff" * (256 - len(trns))
    table = [palette[i * 3 : i * 3 + 3] + alphas[i : i + 1] for i in range(len(palette) // 3)]
    per = 8 // depth
    mask = (1 << depth) - 1
    rgba = bytearray()
    for y in range(h):
      row = raw[y * stride : (y + 1) * stride]
      if depth == 8:
        idx = row
      else:
        idx = [(row[x // per] >> (8 - depth * (x % per + 1))) & mask for x in range(w)]

      rgba += b"".join(table[i] for i in idx)

    return w, h, rgba

  rgba = bytearray(w * h * 4)
  if color == 6:
    rgba[:] = raw
  elif color == 2:
    for c in range(3):
      rgba[c::4] = raw[c::3]

    rgba[3::4] = b"\xff" * (w * h)
  elif color == 4:
    for c in range(3):
      rgba[c::4] = raw[0::2]

    rgba[3::4] = raw[1::2]
  else:
    for c in range(3):
      rgba[c::4] = raw

    rgba[3::4] = b"\xff" * (w * h)

  return w, h, rgba


def unfiltered(data: bytes, stride: int, h: int, bpp: int) -> bytearray:
  """The scanlines of `data` without their filter bytes, each filter undone (PNG spec §9)."""
  out = bytearray(stride * h)
  prev = bytearray(stride)
  for y in range(h):
    kind = data[y * (stride + 1)]
    row = bytearray(data[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
    if kind == 1:
      for i in range(bpp, stride):
        row[i] = (row[i] + row[i - bpp]) & 255
    elif kind == 2:
      row = bytearray((a + b) & 255 for a, b in zip(row, prev))
    elif kind == 3:
      for i in range(stride):
        row[i] = (row[i] + ((row[i - bpp] if i >= bpp else 0) + prev[i]) // 2) & 255
    elif kind == 4:
      for i in range(stride):
        a, b = (row[i - bpp] if i >= bpp else 0), prev[i]
        c = prev[i - bpp] if i >= bpp else 0
        p = a + b - c
        pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
        row[i] = (row[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255

    out[y * stride : (y + 1) * stride] = row
    prev = row

  return out


def encode(n: int, rgba: bytes) -> bytes:
  """An `n` × `n` RGBA PNG, unfiltered."""

  def chunk(kind: bytes, body: bytes) -> bytes:
    return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

  rows = b"".join(b"\0" + rgba[y * n * 4 : (y + 1) * n * 4] for y in range(n))
  head = struct.pack(">IIBBBBB", n, n, 8, 6, 0, 0, 0)
  body = chunk(b"IHDR", head) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")
  return PNG + body


# MARK: Command line


def main():
  ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
  ap.add_argument("command", choices=(*OS, "feed", "feed-key"))
  ap.add_argument("files", nargs="*", help="dmg: the .app; feed: [<platform>=]<file>, each")
  ap.add_argument("--manifest", type=Path, default=Path("Cargo.toml"), help="the app's Cargo.toml")
  ap.add_argument(
    "--target", help="the target triple, by default this machine's arch on the command's OS"
  )
  ap.add_argument(
    "--bin", type=Path, help="the built binary, by default <target dir>/<triple>/dist/<bin>"
  )
  ap.add_argument("--out", type=Path, help="where the files go, by default <target dir>/distrib")
  ap.add_argument("--key", type=Path, help="feed: the private key's file, else $UPDATE_KEY")
  ap.add_argument("--url", help="feed: where the files will be served, {version} the version")
  ap.add_argument(
    "--public-key", action="store_true", help="feed: print the key's public half in hex"
  )
  ap.add_argument(
    "--repo", help="feed-key: the GitHub repo of the secret, by default the current one"
  )
  args = ap.parse_intermixed_args()
  # The OS modules import this file as `pack`: the same module, not a second copy.
  sys.modules.setdefault("pack", sys.modules[__name__])
  if args.command == "feed-key":
    feed_key(args.repo)
    return

  if args.command == "feed":
    key = args.key.read_text() if args.key else os.environ.get("UPDATE_KEY")
    if not key:
      ap.error("feed needs the key: --key <file> or $UPDATE_KEY")

    if args.public_key:
      print(public_key(pem(key)))
      return

    if not args.url or not args.files:
      ap.error("feed needs --url and the files")

  cfg = load(args.manifest)
  out = args.out or cfg.target / "distrib"
  if args.command == "feed":
    feed(pem(key), cfg.version, args.url, args.files, out)
    return

  if args.command == "dmg":
    import pack_macos

    if len(args.files) > 1:
      ap.error("dmg takes one .app")

    pack_macos.dmg(cfg, Path(args.files[0]) if args.files else out / f"{cfg.name}.app", out)
    return

  if args.files:
    ap.error(f"{args.command} takes no files")

  target = args.target or f"{arch(host.machine())}-{OS[args.command]}"
  exe = cfg.bin + (".exe" if "windows" in target else "")
  binary = args.bin or cfg.target / target / "dist" / exe
  if not binary.is_file():
    sys.exit(f"pack: no binary at {binary}: build it, or give --bin")

  out.mkdir(parents=True, exist_ok=True)
  if args.command == "app":
    import pack_macos

    pack_macos.app(cfg, binary, out)
  elif args.command == "setup":
    import pack_windows

    print(pack_windows.setup(cfg, binary, arch(target), out))
  else:
    import pack_linux

    print(getattr(pack_linux, args.command)(cfg, binary, arch(target), out))


if __name__ == "__main__":
  main()
