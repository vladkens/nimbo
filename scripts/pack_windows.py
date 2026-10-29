# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
"""Makes an app's Windows installer, `<Name>-v<version>-windows-<x64|arm64>-setup.exe`, by Inno.

A per-user install, no administrator: `%LOCALAPPDATA%\\Programs\\<Name>` always, the exe as
`<Name>.exe` (and LICENSE.txt when the app has a LICENSE), a Start menu entry, the uninstaller in
Settings › Apps. `AppId` is the config's `windows.app-id`, else a GUID made from the app's id, the
same in every version, so a newer setup replaces the install:
the updater (src/update.rs) runs it as `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CURRENTUSER`
once the app has quit, so it neither waits for nor closes the app, and starts nothing; an
interactive install offers to start it. The uninstaller also removes the app's launch at login
(nimbo's value named by its id under HKCU `...\\CurrentVersion\\Run`), which would point at a
deleted exe.

The setup's own icon is an .ico made from the app's PNG here, in the standard library: the PNG
decoded, cut to the square around its visible pixels (macOS icons keep a margin around the tile,
which would show small on Windows), averaged down to 16–256 px and stored as PNG entries
(Vista+). A PNG it can't decode (16-bit, interlaced) goes in as it is, a single entry.

ISCC is Inno Setup 6's compiler: in `%ProgramFiles(x86)%\\Inno Setup 6` (preinstalled on GitHub's
Windows runners) or on PATH.
"""

from __future__ import annotations

import os
import re
import shutil
import struct
import sys
import tempfile
import uuid
from pathlib import Path

from pack import Config, decode, encode, run

NAMES = {"x86_64": "x64", "aarch64": "arm64"}
# Inno's architecture identifiers: x64compatible takes x64 Windows and ARM64's x64 emulation.
ARCHES = {"x86_64": "x64compatible", "aarch64": "arm64"}
ICO = (16, 24, 32, 48, 64, 256)
RUN = r"Software\Microsoft\Windows\CurrentVersion\Run"
APPROVED = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"


def setup(cfg: Config, binary: Path, arch: str, out_dir: Path) -> Path:
  """Compiles the installer of `binary` (built for `arch`, x86_64 or aarch64) into `out_dir`."""
  if not binary.is_file() or binary.stat().st_size == 0:
    sys.exit(f"pack: no app to install at {binary}: build it first")

  iscc = compiler()
  out_dir.mkdir(parents=True, exist_ok=True)
  base = f"{cfg.name}-v{cfg.version}-windows-{NAMES[arch]}-setup"

  with tempfile.TemporaryDirectory() as tmp:
    icon = Path(tmp) / f"{cfg.name}.ico"
    icon.write_bytes(ico(cfg.icon.read_bytes()))
    iss = Path(tmp) / f"{cfg.name}.iss"
    license = cfg.root / "LICENSE"
    text = script(cfg, binary.resolve(), arch, out_dir.resolve(), base, icon, license)
    # Inno reads UTF-8 with its byte-order mark: a name may not be ASCII.
    iss.write_text(text, encoding="utf-8-sig")
    run(iscc, "/Q", iss)

  return out_dir / f"{base}.exe"


def compiler() -> str:
  """ISCC's path; exits naming how to install it when there is none."""
  where = Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)"))
  iscc = where / "Inno Setup 6" / "ISCC.exe"
  if iscc.is_file():
    return str(iscc)

  found = shutil.which("iscc")
  if found:
    return found

  sys.exit(f"pack: no Inno Setup 6 at {iscc} or ISCC on PATH; install it: choco install innosetup")


def app_id(cfg: Config) -> str:
  """The installer's AppId, the same in every version: `windows.app-id` of the config, else a
  GUID made from the app's id. Never changed: an update and the uninstall find the app by it.

  >>> from types import SimpleNamespace as N
  >>> app_id(N(id="me.vladkens.wavo", windows={}))
  '{a754b18f-b91c-5def-b91e-a32c2017c75f}'
  >>> app_id(N(id="x", windows={"app-id": "314a56a8-8a8b-4c18-954d-b930203f086d"}))
  '{314a56a8-8a8b-4c18-954d-b930203f086d}'
  """
  given = cfg.windows.get("app-id")
  guid = uuid.UUID(given) if given else uuid.uuid5(uuid.NAMESPACE_DNS, cfg.id)
  return "{" + str(guid) + "}"


def publisher(cfg: Config) -> str:
  """Who Settings › Apps names: the config's publisher, else the id's owner (`me.vladkens.x`)."""
  if cfg.publisher:
    return cfg.publisher

  parts = cfg.id.split(".")
  return parts[1] if len(parts) > 2 else parts[0]


def script(
  cfg: Config, binary: Path, arch: str, out_dir: Path, base: str, icon: Path, license: Path
) -> str:
  r"""The .iss for the installer `base` in `out_dir`.

  >>> from types import SimpleNamespace as N
  >>> cfg = N(id="me.vladkens.counter", name="Counter", version="1.2.0-3-gabc", comment="Counts",
  ...   windows={}, publisher="")
  >>> iss = script(cfg, Path("c.exe"), "aarch64", Path("out"), "b", Path("i.ico"), Path("-"))
  >>> keys = ("AppId", "AppVersion", "VersionInfo", "Priv", "DefaultDir", "Arch", "Close", "Source")
  >>> print("\n".join(l for l in iss.splitlines() if l.startswith(keys)))
  AppId={{e65f79c2-5031-5421-a3d1-3c434f820e5d}
  AppVersion=1.2.0-3-gabc
  VersionInfoVersion=1.2.0
  PrivilegesRequired=lowest
  DefaultDirName={userpf}\Counter
  ArchitecturesAllowed=arm64
  ArchitecturesInstallIn64BitMode=arm64
  CloseApplications=no
  Source: "c.exe"; DestDir: "{app}"; DestName: "Counter.exe"; Flags: ignoreversion
  >>> "Flags: nowait postinstall skipifsilent" in iss, "Mutex" in iss or "LICENSE" in iss
  (True, False)
  """
  exe = f"{cfg.name}.exe"
  name, app = esc(cfg.name), r"{app}" + "\\" + esc(exe)
  numeric = re.match(r"\d+(\.\d+){0,3}", cfg.version)
  files = [(binary, exe)] + ([(license.resolve(), "LICENSE.txt")] if license.is_file() else [])
  files = [f'Source: {q(str(src))}; DestDir: "{{app}}"; DestName: {q(to)}' for src, to in files]
  entry = q(r"{autoprograms}" + "\\" + name, False)
  # A Pascal string: an apostrophe is doubled.
  value = "'" + cfg.id.replace("'", "''") + "'"
  lines = [
    "[Setup]",
    f"AppId={esc(app_id(cfg))}",
    f"AppName={name}",
    f"AppVersion={esc(cfg.version)}",
    *([f"VersionInfoVersion={numeric.group()}"] if numeric else []),
    f"AppPublisher={esc(publisher(cfg))}",
    f"AppComments={esc(cfg.comment)}",
    "PrivilegesRequired=lowest",
    # %LOCALAPPDATA%\Programs\<Name>.
    rf"DefaultDirName={{userpf}}\{name}",
    "DisableDirPage=yes",
    "DisableProgramGroupPage=yes",
    f"ArchitecturesAllowed={ARCHES[arch]}",
    f"ArchitecturesInstallIn64BitMode={ARCHES[arch]}",
    f"UninstallDisplayName={name}",
    f"UninstallDisplayIcon={app}",
    f"SetupIconFile={esc(str(icon))}",
    # The updater runs it once the app has quit; an interactive install over a running copy asks
    # to close it by hand.
    "CloseApplications=no",
    "RestartApplications=no",
    f"OutputDir={esc(str(out_dir))}",
    f"OutputBaseFilename={esc(base)}",
    "Compression=lzma2",
    "SolidCompression=yes",
    "WizardStyle=modern",
    "",
    "[Files]",
    *(f"{line}; Flags: ignoreversion" for line in files),
    "",
    "[Icons]",
    f"Name: {entry}; Filename: {q(app, False)}; Comment: {q(cfg.comment)}",
    "",
    "; An interactive install only: a silent one is the updater's, which starts the app itself.",
    "[Run]",
    # A constant's argument ends at a comma or a brace: those %-encoded.
    f'Filename: {q(app, False)}; Description: "{{cm:LaunchProgram,{arg(cfg.name)}}}"; '
    + "Flags: nowait postinstall skipifsilent",
    "",
    "; nimbo's launch at login, which would start a deleted exe.",
    "[Code]",
    "procedure CurUninstallStepChanged(Step: TUninstallStep);",
    "begin",
    "  if Step = usPostUninstall then begin",
    f"    RegDeleteValue(HKCU, '{RUN}', {value});",
    f"    RegDeleteValue(HKCU, '{APPROVED}', {value});",
    "  end;",
    "end;",
  ]
  return "\n".join(lines) + "\n"


def esc(s: str) -> str:
  """`s` in an .iss value, where a brace starts a constant: doubled.

  >>> esc("{a}")
  '{{a}'
  """
  return s.replace("{", "{{")


def q(s: str, plain: bool = True) -> str:
  """`s` as a quoted parameter, its quotes doubled; escaped as well when `plain` (no constants).

  >>> q('say "hi" {x}')
  '"say ""hi"" {{x}"'
  """
  return '"' + (esc(s) if plain else s).replace('"', '""') + '"'


def arg(s: str) -> str:
  """`s` as a constant's argument: `%`, `,`, `|` and `}` %-encoded, `{` doubled.

  >>> arg("A, {b}")
  'A%2c {{b%7d'
  """
  for c in "%,|}":
    s = s.replace(c, f"%{ord(c):02x}")

  return esc(s).replace('"', '""')


# MARK: The icon


def ico(png: bytes) -> bytes:
  """An .ico of PNG entries from the icon `png`: its visible square at each size of ICO."""
  image = decode(png)
  if image is None:
    return icon_file([(0, png)])

  w, h, rgba = image
  left, top, side = visible(w, h, rgba)
  big = resized(w, h, rgba, left, top, side, 256)
  entries = [(n, encode(n, resized(256, 256, big, 0, 0, 256, n) if n < 256 else big)) for n in ICO]
  return icon_file(entries)


def icon_file(entries: list[tuple[int, bytes]]) -> bytes:
  """ICONDIR, the entries' records and their PNGs; a side of 256 or more is written 0."""
  out, at = [struct.pack("<HHH", 0, 1, len(entries))], 6 + 16 * len(entries)
  for n, data in entries:
    side = n if 0 < n < 256 else 0
    out.append(struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(data), at))
    at += len(data)

  return b"".join(out + [data for _, data in entries])


def visible(w: int, h: int, rgba: bytearray) -> tuple[int, int, int]:
  """The square around the pixels that show, centred on them: its left, top and side.

  >>> visible(4, 2, bytearray([0] * 4 + [9, 9, 9, 255] + [0] * 24))
  (1, 0, 1)
  """
  alpha = rgba[3::4]
  rows = [y for y in range(h) if max(alpha[y * w : (y + 1) * w]) > 8]
  cols = [x for x in range(w) if max(alpha[x::w]) > 8]
  if not rows:
    return 0, 0, max(w, h)

  side = max(rows[-1] - rows[0], cols[-1] - cols[0]) + 1
  return (cols[0] + cols[-1] + 1) // 2 - side // 2, (rows[0] + rows[-1] + 1) // 2 - side // 2, side


def resized(w: int, h: int, rgba: bytearray, left: int, top: int, side: int, n: int) -> bytes:
  """The square at `left`, `top` of `side` pixels as `n` × `n`, each the average of the pixels it
  covers weighted by their alpha (straight colours would bleed the transparent black); outside
  the image is transparent."""
  # The output column or row of each source one in the square, and how many fall in each.
  to = [i * n // side for i in range(side)]
  count = [to.count(i) for i in range(n)]
  sums = [0] * (n * n * 4)
  for y in range(max(0, top), min(h, top + side)):
    oy = to[y - top] * n
    for x in range(max(0, left), min(w, left + side)):
      at = (y * w + x) * 4
      a = rgba[at + 3]
      if not a:
        continue

      o = (oy + to[x - left]) * 4
      sums[o] += rgba[at] * a
      sums[o + 1] += rgba[at + 1] * a
      sums[o + 2] += rgba[at + 2] * a
      sums[o + 3] += a

  out = bytearray(n * n * 4)
  for i in range(n * n):
    a = sums[i * 4 + 3]
    if not a:
      continue

    out[i * 4 : i * 4 + 3] = bytes(min(255, round(sums[i * 4 + c] / a)) for c in range(3))
    out[i * 4 + 3] = min(255, round(a / (count[i // n] * count[i % n])))

  return bytes(out)
