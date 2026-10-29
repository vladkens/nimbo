# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
"""Packs a Linux build into its release files: the AppImage and the .deb.

- `appimage(cfg, binary, arch, out_dir)`: `<Name>-v<version>-linux-<arch>.AppImage`, an AppDir
  with the binary, AppRun, the .desktop entry and the icon, packed by appimagetool with the
  static type2 runtime (no libfuse2 needed). Both are pinned below, downloaded into the app's
  target/appimage once and checked by their SHA-256. A nimbo app links only glibc and loads GTK
  and the rest at run time, so nothing else goes in.
- `deb(cfg, binary, arch, out_dir)`: `<package>_<version>_<amd64|arm64>.deb` by dpkg-deb:
  /usr/bin/<bin>, /usr/share/applications/<id>.desktop, the icon in hicolor at 512 and 256 px
  (scaled here from the 1024 px PNG), the app's LICENSE as /usr/share/doc/<package>/copyright.
  The config's `linux` table names the package (`deb-package`, else the binary's name) and lists
  its `deb-depends` and `deb-recommends`.

The .desktop entry is named by the app id, nimbo's X11 WM_CLASS and Wayland app id, so desktops
find it, and its icon, from the window. Ported from wavo-app's scripts/dist.py.
"""

from __future__ import annotations

import hashlib
import os
import platform
import shutil
import struct
import tempfile
import urllib.request
import zlib
from pathlib import Path

from pack import Config, run

APPIMAGETOOL = "https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-{arch}.AppImage"
RUNTIME = "https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-{arch}"
SHA256 = {
  "appimagetool-x86_64": "ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0",
  "appimagetool-aarch64": "f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158",
  "runtime-x86_64": "2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d",
  "runtime-aarch64": "00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444",
}
DEB_ARCH = {"x86_64": "amd64", "aarch64": "arm64"}
# The hicolor sizes of the .deb's icon, halved down from the 1024 px PNG.
SIZES = (512, 256)
PNG = b"\x89PNG\r\n\x1a\n"
# The samples per pixel of each PNG colour type: grey, RGB, palette, grey and alpha, RGBA.
CHANNELS = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}


def desktop(cfg: Config) -> str:
  """The .desktop entry: the app started by its binary's name, on the PATH."""
  lines = ["[Desktop Entry]", "Type=Application", f"Name={cfg.name}"]
  if cfg.comment:
    lines.append(f"Comment={cfg.comment}")

  lines += [f"Exec={cfg.bin}", f"Icon={cfg.id}", f"StartupWMClass={cfg.id}", "StartupNotify=true"]
  categories = cfg.linux.get("categories", [])
  if categories:
    lines.append("Categories=" + "".join(f"{c};" for c in categories))

  lines.append("Terminal=false")
  return "\n".join(lines) + "\n"


def deb_version(version: str) -> str:
  """The Cargo version as Debian orders it: a pre-release sorts before its release.

  >>> deb_version("1.2.0-beta.1")
  '1.2.0~beta.1'
  >>> deb_version("1.2.0")
  '1.2.0'
  """
  return version.replace("-", "~", 1)


# MARK: AppImage


def fetch(url: str, path: Path) -> Path:
  """`url` into `path`, once, checked against SHA256 by the file's name."""
  if not path.exists():
    path.parent.mkdir(parents=True, exist_ok=True)
    part = path.with_suffix(".part")
    with urllib.request.urlopen(url) as r, open(part, "wb") as f:
      shutil.copyfileobj(r, f)

    part.rename(path)

  digest = hashlib.sha256(path.read_bytes()).hexdigest()
  if digest != SHA256[path.name]:
    path.unlink()
    raise SystemExit(f"{path.name}: SHA-256 {digest}, expected {SHA256[path.name]}")

  path.chmod(0o755)
  return path


def appimage(cfg: Config, binary: Path, arch: str, out_dir: Path) -> Path:
  # The tool runs here; the runtime is the AppImage's own, of its arch.
  host = platform.machine().lower()
  host = {"amd64": "x86_64", "arm64": "aarch64"}.get(host, host)
  tools = cfg.target / "appimage"
  tool = fetch(APPIMAGETOOL.format(arch=host), tools / f"appimagetool-{host}")
  runtime = fetch(RUNTIME.format(arch=arch), tools / f"runtime-{arch}")

  out_dir.mkdir(parents=True, exist_ok=True)
  out = out_dir / f"{cfg.name}-v{cfg.version}-linux-{arch}.AppImage"
  with tempfile.TemporaryDirectory() as tmp:
    appdir = Path(tmp) / f"{cfg.name}.AppDir"
    (appdir / "usr" / "bin").mkdir(parents=True)
    shutil.copy2(binary, appdir / "usr" / "bin" / cfg.bin)
    (appdir / "AppRun").symlink_to(f"usr/bin/{cfg.bin}")
    (appdir / f"{cfg.id}.desktop").write_text(desktop(cfg))
    shutil.copy2(cfg.root / cfg.icon, appdir / f"{cfg.id}.png")
    (appdir / ".DirIcon").symlink_to(f"{cfg.id}.png")

    # Runs without FUSE (CI has none): the tool unpacks itself.
    env = {**os.environ, "APPIMAGE_EXTRACT_AND_RUN": "1", "ARCH": arch}
    run(tool, "--no-appstream", "--runtime-file", runtime, appdir, out, env=env)

  return out


# MARK: PNG


def png_read(path: Path) -> tuple[int, int, bytearray] | None:
  """The size and RGBA pixels of a PNG, not interlaced, 8-bit or a palette's; else None."""
  data = path.read_bytes()
  if data[:8] != PNG:
    return None

  pos, chunks, idat = 8, {}, []
  while pos < len(data):
    n, kind = struct.unpack(">I4s", data[pos : pos + 8])
    chunks[kind] = data[pos + 8 : pos + 8 + n]
    pos += 12 + n
    if kind == b"IDAT":
      idat.append(chunks[kind])

  w, h, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", chunks[b"IHDR"])
  if color not in CHANNELS or interlace or depth != 8 and color != 3:
    return None

  bits = CHANNELS[color] * depth
  bpp, stride = max(1, bits // 8), (w * bits + 7) // 8
  raw = zlib.decompress(b"".join(idat))
  px = bytearray(h * stride)
  prev = bytearray(stride)
  # Undoes each row's filter (PNG spec, 9.2): Sub, Up, Average, Paeth.
  for y in range(h):
    at = y * (stride + 1)
    kind, row = raw[at], bytearray(raw[at + 1 : at + 1 + stride])
    if kind == 1:
      for i in range(bpp, stride):
        row[i] = (row[i] + row[i - bpp]) & 255
    elif kind == 2:
      row = bytearray((x + b) & 255 for x, b in zip(row, prev))
    elif kind == 3:
      for i in range(stride):
        row[i] = (row[i] + ((row[i - bpp] if i >= bpp else 0) + prev[i]) // 2) & 255
    elif kind == 4:
      for i in range(stride):
        a, b, c = (row[i - bpp], prev[i], prev[i - bpp]) if i >= bpp else (0, prev[i], 0)
        pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
        row[i] = (row[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255

    px[y * stride : (y + 1) * stride] = row
    prev = row

  if color == 3:
    # Indexes, `depth` bits each from the high ones, a row padded to a byte; alpha from tRNS.
    plte, trns = chunks[b"PLTE"], chunks.get(b"tRNS", b"")
    alpha = trns + b"\xff" * (len(plte) // 3)
    table = [plte[3 * i : 3 * i + 3] + alpha[i : i + 1] for i in range(len(plte) // 3)]
    shifts, mask = range(8 - depth, -1, -depth), (1 << depth) - 1
    rows = (px[y * stride : (y + 1) * stride] for y in range(h))
    index = px
    if depth < 8:
      index = [i for r in rows for i in [b >> s & mask for b in r for s in shifts][:w]]

    return w, h, bytearray(b"".join(table[i] for i in index))

  # Grey, grey and alpha, RGB, RGBA: the channel each of R, G, B, A comes from.
  n, rgba = CHANNELS[color], bytearray(b"\xff" * (w * h * 4))
  for c, src in enumerate({0: (0, 0, 0), 2: (0, 1, 2), 4: (0, 0, 0, 1), 6: (0, 1, 2, 3)}[color]):
    rgba[c::4] = px[src::n]

  return w, h, rgba


def png_write(path: Path, w: int, h: int, px: bytearray) -> None:
  def chunk(kind: bytes, body: bytes) -> bytes:
    return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

  rows = b"".join(b"\0" + px[y * w * 4 : (y + 1) * w * 4] for y in range(h))
  head = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
  body = chunk(b"IHDR", head) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")
  path.write_bytes(PNG + body)


def halve(w: int, h: int, px: bytearray) -> tuple[int, int, bytearray]:
  """RGBA pixels at half the size, each the alpha-weighted mean of four: clear ones add no colour."""
  w2, h2 = w // 2, h // 2
  out = bytearray(w2 * h2 * 4)
  for y in range(h2):
    top, low = 2 * y * w * 4, (2 * y + 1) * w * 4
    for x in range(w2):
      quad = (top + 8 * x, top + 8 * x + 4, low + 8 * x, low + 8 * x + 4)
      alpha = [px[i + 3] for i in quad]
      total = sum(alpha)
      o = (y * w2 + x) * 4
      out[o + 3] = (total + 2) // 4
      if not total:
        continue

      for c in range(3):
        out[o + c] = (sum(px[i + c] * a for i, a in zip(quad, alpha)) + total // 2) // total

  return w2, h2, out


# MARK: deb


def icons(src: Path, root: Path, id: str) -> None:
  """The icon in hicolor under `root`, halved down to SIZES; a PNG this can't read as is."""
  png = png_read(src)
  if png is None or png[0] != png[1]:
    print(f"pack: {src.name}: not a square PNG this reads (16-bit, interlaced); not scaled")
    for dir in ("icons/hicolor/1024x1024/apps", "pixmaps"):
      put(root / "usr/share" / dir / f"{id}.png", src.read_bytes())

    return

  w, h, px = png
  while w > SIZES[-1]:
    w, h, px = halve(w, h, px)
    if w in SIZES:
      dir = root / f"usr/share/icons/hicolor/{w}x{w}/apps"
      dir.mkdir(parents=True, exist_ok=True)
      png_write(dir / f"{id}.png", w, h, px)


def put(path: Path, data: bytes) -> None:
  path.parent.mkdir(parents=True, exist_ok=True)
  path.write_bytes(data)


def deb(cfg: Config, binary: Path, arch: str, out_dir: Path) -> Path:
  pkg = cfg.linux.get("deb-package") or cfg.bin.lower().replace("_", "-")
  version = deb_version(cfg.version)
  out_dir.mkdir(parents=True, exist_ok=True)
  out = out_dir / f"{pkg}_{version}_{DEB_ARCH[arch]}.deb"

  with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp) / pkg
    put(root / "usr/bin" / cfg.bin, binary.read_bytes())
    put(root / f"usr/share/applications/{cfg.id}.desktop", desktop(cfg).encode())
    icons(cfg.root / cfg.icon, root, cfg.id)
    license = cfg.root / "LICENSE"
    if license.is_file():
      put(root / f"usr/share/doc/{pkg}/copyright", license.read_bytes())

    # dpkg-deb takes the modes as they are: 0755 for the directories and the binary.
    size = 0
    for dir, _, files in os.walk(root):
      os.chmod(dir, 0o755)
      for name in files:
        path = Path(dir) / name
        path.chmod(0o755 if path == root / "usr/bin" / cfg.bin else 0o644)
        size += path.stat().st_size

    # The package's first author, as Debian wants one maintainer; else the publisher or the name.
    authors = next(iter(cfg.authors), "") or cfg.publisher or cfg.name
    control = {
      "Package": pkg,
      "Version": version,
      "Architecture": DEB_ARCH[arch],
      "Maintainer": authors,
      "Installed-Size": str(size // 1024 + 1),
      "Depends": ", ".join(cfg.linux.get("deb-depends", [])),
      "Recommends": ", ".join(cfg.linux.get("deb-recommends", [])),
      "Section": "utils",
      "Priority": "optional",
      "Description": cfg.comment or cfg.name,
    }
    debian = root / "DEBIAN"
    debian.mkdir()
    debian.chmod(0o755)
    (debian / "control").write_text("".join(f"{k}: {v}\n" for k, v in control.items() if v))
    # xz, not Ubuntu's default zstd, which older dpkg can't read.
    run("dpkg-deb", "-Zxz", "--root-owner-group", "--build", root, out)

  return out
