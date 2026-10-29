# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
"""Makes an app's release files from its Cargo.toml: the macOS app and DMG, the Windows
installer, the AppImage and .deb, and the signed update feed (src/update.rs).

The config is the package's `[package.metadata.nimbo]` (`id`, `name`, `icon`, a 1024 px PNG,
`comment`, the tables `macos`, `linux` and `windows`), its version and binary, read by `cargo
metadata --no-deps`, which compiles nothing: a plain `python3` (3.9, macOS's own) with the standard
library runs this. The binary is built already, by default `<target dir>/<triple>/dist/<bin>`, the
target this machine's arch on the command's OS. The files, in `--out` (`<target dir>/distrib`), are
`<Name>-v<version>-<platform>.<ext>`, or `<Name>-<label>-<platform>.<ext>` with `--label` (`dev`,
which rust-publish renames `-vX.Y.Z` on a release). `app` writes `<target dir>/<Name>.app`, which
`dmg` packs, or the one `--app` gives, never signed again. `dist` runs this OS's steps: `app` and
`dmg`, `setup`, or `appimage` and `deb`. Locally codesign takes no timestamp, so a Developer ID
signs offline; in CI (`$CI`) it does, and macos-sign signs with one anyway.

The macOS sign job compiles nothing: it checks nimbo out at the rev the app's Cargo.lock pins
(`git+https://github.com/vladkens/nimbo…#<sha>`) and runs `app` and `dmg` from there, which sign
with its Developer ID; vladkens/actions' macos-sign notarizes and staples the DMG. An app's
Makefile finds nimbo's checkout by `cargo metadata` and runs a line per target:

  NIMBO = $(shell cargo metadata --format-version 1 --locked | python3 -c 'import json, os, sys; \
    print(next(os.path.dirname(p["manifest_path"]) for p in json.load(sys.stdin)["packages"] \
    if p["name"] == "nimbo"))')
  dist:
  	cargo build --locked --profile dist --target aarch64-apple-darwin
  	python3 $(NIMBO)/scripts/pack.py dist --label dev --target aarch64-apple-darwin

`feed` writes update.txt: `version <v>`, a line per file, `<platform> <size> <sha512> <url>`, and
last `sig <hex>`, the Ed25519 signature of every byte before it. A file's platform is in its name
(`linux-aarch64`, `windows-x64`) or given, `<platform>=<file>`; its url is `--url` (`{version}` the
version) and its name. The key, `--key <file>` or `$UPDATE_KEY`, is its seed in hex or a PEM.
`feed-key` makes the one key, in memory only: the repo's secret UPDATE_KEY (gh), the private key on
the clipboard to keep (Bitwarden), the public key printed in hex, the app's `Feed::key`. Both need
OpenSSL 3 (macOS's own openssl is LibreSSL).

python3 scripts/pack.py dist|app|setup|appimage|deb [--target <triple>] [--bin <path>] [--label <l>] [--out <dir>] [--manifest <Cargo.toml>]
python3 scripts/pack.py dmg [--app <Name>.app] [--target <triple>] [--label <l>] [--out <dir>] [--manifest <Cargo.toml>]
python3 scripts/pack.py feed [--key <file>] --url <base> [--out <dir>] [<platform>=]<file>...
python3 scripts/pack.py feed [--key <file>] --public-key
python3 scripts/pack.py feed-key [--repo <owner/name>]
"""

from __future__ import annotations

import argparse
import base64
import functools
import hashlib
import json
import os
import platform as host
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import uuid
from dataclasses import dataclass
from pathlib import Path

# The triple's OS of each command, for the default target.
OS = {
  "app": "apple-darwin",
  "dmg": "apple-darwin",
  "setup": "pc-windows-msvc",
  "appimage": "unknown-linux-gnu",
  "deb": "unknown-linux-gnu",
}
# The commands `dist` runs on each OS, in order.
DIST = {"darwin": ("app", "dmg"), "win32": ("setup",), "linux": ("appimage", "deb")}


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
  label: str | None = None


def stem(cfg: Config) -> str:
  """A file's name before its platform: `<Name>-<label>`, else `<Name>-v<version>`.

  >>> cfg = Config("me.a", "A", Path("i.png"), "", "1.2.0", "a", {}, {}, {}, "", [], Path("."),
  ...   Path("target"))
  >>> stem(cfg), stem(Config(**{**cfg.__dict__, "label": "dev"}))
  ('A-v1.2.0', 'A-dev')
  """
  return f"{cfg.name}-{cfg.label or 'v' + cfg.version}"


def run(*args, **kw) -> subprocess.CompletedProcess:
  return subprocess.run([str(a) for a in args], check=True, **kw)


def load(manifest: Path) -> Config:
  if not shutil.which("cargo"):
    sys.exit("pack: cargo not found: `cargo metadata` reads the config")

  args = ["--format-version", "1", "--no-deps", "--manifest-path", manifest]
  return parse(json.loads(run("cargo", "metadata", *args, capture_output=True).stdout), manifest)


def parse(meta: dict, manifest: Path) -> Config:
  """The config of the package at `manifest` in `cargo metadata`'s output.

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
    # The config's, else the first author without the mail.
    publisher=nimbo.get("publisher") or next(iter(authors), "").split(" <")[0],
    authors=authors,
    root=manifest.parent,
    target=Path(meta.get("target_directory") or manifest.parent / "target"),
  )


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


# MARK: macOS

KEYS = {"accessory", "usage", "plist", "entitlements"}


def info(cfg: Config, minos: str) -> dict:
  """The Info.plist: the bundle from the config, then its `usage` strings and `plist` keys over it.

  >>> mac = {"accessory": True, "usage": {"NSMicrophoneUsageDescription": "m"}, "plist": {"X": 1}}
  >>> cfg = Config("me.a", "A", Path("i.png"), "", "1.2.0-beta.1", "a", mac, {}, {}, "", [],
  ...   Path("."), Path("target"))
  >>> p = info(cfg, "14.0")
  >>> p["CFBundleExecutable"], p["CFBundleVersion"], p["LSUIElement"], p["X"]
  ('a', '1.2.0', True, 1)
  >>> p["LSMinimumSystemVersion"]
  '14.0'
  """
  bad = set(cfg.macos) - KEYS
  if bad:
    bad, known = ", ".join(sorted(bad)), ", ".join(sorted(KEYS))
    sys.exit(f"pack: unknown [package.metadata.nimbo.macos] keys {bad}: one of {known}")

  # Both versions are numbers and dots only: 1.2.0 of 1.2.0-beta.1.
  version = re.match(r"\d+(\.\d+)*", cfg.version)[0]
  plist = {
    "CFBundleIdentifier": cfg.id,
    "CFBundleName": cfg.name,
    "CFBundleDisplayName": cfg.name,
    "CFBundleExecutable": cfg.bin,
    "CFBundleIconFile": cfg.name,
    "CFBundlePackageType": "APPL",
    "CFBundleShortVersionString": version,
    "CFBundleVersion": version,
    "CFBundleInfoDictionaryVersion": "6.0",
    "CFBundleDevelopmentRegion": "en",
    "LSMinimumSystemVersion": minos,
    "NSHighResolutionCapable": True,
  }
  if cfg.macos.get("accessory"):
    plist["LSUIElement"] = True

  return {**plist, **cfg.macos.get("usage", {}), **cfg.macos.get("plist", {})}


def minos(binary: Path) -> str:
  out = run("vtool", "-show-build", binary, capture_output=True, text=True).stdout
  return next(ln.split()[1] for ln in out.splitlines() if ln.split()[:1] == ["minos"])


def icns(png: Path, out: Path) -> None:
  with tempfile.TemporaryDirectory(suffix=".iconset") as tmp:
    for size in (16, 32, 128, 256, 512):
      for scale, suffix in ((1, ""), (2, "@2x")):
        px = str(size * scale)
        icon = Path(tmp) / f"icon_{size}x{size}{suffix}.png"
        run("sips", "-z", px, px, png, "--out", icon, capture_output=True)

    run("iconutil", "-c", "icns", "-o", out, tmp)


def identities(valid: bool) -> list[str]:
  args = ["find-identity", *(["-v"] if valid else []), "-p", "codesigning"]
  out = run("security", *args, capture_output=True, text=True).stdout
  return [ln for ln in out.splitlines() if '"' in ln]


def make_identity(name: str) -> None:
  """A self-signed code signing certificate in the login keychain, codesign allowed to use it."""
  with tempfile.TemporaryDirectory() as tmp:
    key, cert, p12 = (Path(tmp) / n for n in ("key.pem", "cert.pem", "identity.p12"))
    # macOS's own openssl (LibreSSL): `security import` reads its PKCS#12, not OpenSSL 3's.
    openssl = "/usr/bin/openssl"
    new = ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "3650", "-subj", f"/CN={name}"]
    new += ["-addext", "keyUsage=critical,digitalSignature"]
    new += ["-addext", "extendedKeyUsage=critical,codeSigning"]
    run(openssl, *new, "-keyout", key, "-out", cert, capture_output=True)
    export = ["pkcs12", "-export", "-inkey", key, "-in", cert, "-out", p12]
    run(openssl, *export, "-passout", "pass:nimbo", capture_output=True)
    keychain = Path.home() / "Library/Keychains/login.keychain-db"
    trust = ["-P", "nimbo", "-T", "/usr/bin/codesign"]
    run("security", "import", p12, "-k", keychain, *trust, capture_output=True)

  if not any(f'"{name}"' in ln for ln in identities(False)):
    sys.exit(f'pack: made "{name}", but `security find-identity` does not list it')

  print(f'pack: made the code signing identity "{name}" in the login keychain', file=sys.stderr)


def given() -> tuple[str, str] | None:
  """The release's identity, as codesign takes it and as it is said: $NIMBO_SIGN_IDENTITY (`-` ad
  hoc), else a Developer ID in the keychain by its hash (two, a renewal, are ambiguous by name)."""
  name = os.environ.get("NIMBO_SIGN_IDENTITY")
  if name:
    return name, "ad hoc" if name == "-" else name

  dev = [ln for ln in identities(True) if '"Developer ID Application:' in ln]
  return (dev[0].split()[1], dev[0].split('"')[1]) if dev else None


def identity(cfg: Config) -> tuple[str, str]:
  """What signs the app, as `given` says it: the given one, else ad hoc in CI, else "<Name> Local
  Signing", made once in the login keychain: its designated requirement names the bundle id and
  the certificate, not the code hash, so the permissions macOS grants survive rebuilds."""
  found = given()
  if found:
    return found

  if os.environ.get("CI"):
    return "-", "ad hoc (CI)"

  name = f"{cfg.name} Local Signing"
  if not any(f'"{name}"' in ln for ln in identities(False)):
    make_identity(name)

  return name, name


def stamp(sha: str) -> str:
  """A secure timestamp, which notarization needs, in CI only (macos-sign signs with one again):
  locally a Developer ID signs offline. Never for ad hoc."""
  return "--timestamp" if os.environ.get("CI") and sha != "-" else "--timestamp=none"


def app(cfg: Config, binary: Path) -> Path:
  """`<target dir>/<Name>.app`: the binary, the icns (sips) and the Info.plist, signed by
  `identity` with the hardened runtime and `macos.entitlements`. One codesign fails on (or a
  keychain dialog nobody answers stops) is no bundle, never one signed ad hoc, whose grants would
  last that build only."""
  sha, name = identity(cfg)
  bundle = cfg.target / f"{cfg.name}.app"
  shutil.rmtree(bundle, ignore_errors=True)
  (bundle / "Contents/MacOS").mkdir(parents=True)
  (bundle / "Contents/Resources").mkdir(parents=True)
  shutil.copy2(binary, bundle / "Contents/MacOS" / cfg.bin)
  icns(cfg.icon, bundle / f"Contents/Resources/{cfg.name}.icns")
  (bundle / "Contents/PkgInfo").write_text("APPL????")
  plist = bundle / "Contents/Info.plist"
  plist.write_bytes(plistlib.dumps(info(cfg, minos(binary))))
  run("plutil", "-lint", "-s", plist)
  run("xattr", "-cr", bundle)
  entitlements = cfg.macos.get("entitlements")
  sign = ["--force", "--sign", sha, "--options", "runtime", stamp(sha)]
  sign += ["--entitlements", cfg.root / entitlements] if entitlements else []
  try:
    run("codesign", *sign, bundle, timeout=60)
  except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as e:
    shutil.rmtree(bundle, ignore_errors=True)
    sys.exit(f'pack: codesign with "{name}" failed ({e}): no bundle')

  run("codesign", "--verify", "--strict", bundle)
  print(f"{bundle}: {cfg.name} {cfg.version}, signed: {name}")
  return bundle


def dmg(cfg: Config, bundle: Path, target: str | None, out_dir: Path) -> Path:
  """`<stem>-macos-<arch>.dmg` (UDZO) of `bundle`, as it is signed, and an /Applications link,
  the arch `target`'s, else the bundle's binary's (lipo); the DMG alone signed as `given` says
  unless that is ad hoc."""
  plist = bundle / "Contents/Info.plist"
  if not plist.is_file():
    sys.exit(f"pack: {bundle} is no app bundle")

  exe = bundle / "Contents/MacOS" / plistlib.loads(plist.read_bytes())["CFBundleExecutable"]
  lipo = run("lipo", "-archs", exe, capture_output=True, text=True).stdout.split()
  archs = [target] if target else lipo
  if len(archs) != 1:
    sys.exit(f"pack: {exe} is built for {' and '.join(archs)}: give --target")

  out = out_dir / f"{stem(cfg)}-macos-{arch(archs[0])}.dmg"
  out_dir.mkdir(parents=True, exist_ok=True)
  with tempfile.TemporaryDirectory() as tmp:
    stage = Path(tmp) / cfg.name
    stage.mkdir()
    # ditto keeps the signature, the links and the modes as they are.
    run("ditto", bundle, stage / bundle.name)
    (stage / "Applications").symlink_to("/Applications")
    # hdiutil, which macOS 27 deprecates for `diskutil image`: the runners up to macOS 15 lack that.
    image = ["-volname", cfg.name, "-srcfolder", stage, "-format", "UDZO", "-ov"]
    run("hdiutil", "create", *image, out, capture_output=True)

  found = given()
  signed = found[1] if found and found[0] != "-" else "no"
  if found and found[0] != "-":
    run("codesign", "--force", "--sign", found[0], stamp(found[0]), out, timeout=60)

  print(f"{out}: {cfg.name} {cfg.version}, signed: {signed}")
  return out


# MARK: Windows

NAMES = {"x86_64": "x64", "aarch64": "arm64"}
# Inno's architecture identifiers: x64compatible (6.3+) takes x64 Windows and ARM64's emulation.
ARCHES = {"x86_64": "x64compatible", "aarch64": "arm64"}
RUN = r"Software\Microsoft\Windows\CurrentVersion\Run"
APPROVED = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"


def setup(cfg: Config, binary: Path, arch: str, out_dir: Path) -> Path:
  r"""`<stem>-windows-<x64|arm64>-setup.exe`, compiled by Inno Setup 6's ISCC: a
  per-user install, no administrator, into `%LOCALAPPDATA%\Programs\<Name>` with a Start menu
  entry and the uninstaller in Settings › Apps (which shows the exe's own icon; the setup shows
  Inno's). The updater (src/update.rs) runs it as `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART
  /CURRENTUSER` once the app has quit; an interactive install offers to start it."""
  iscc = compiler()
  out_dir.mkdir(parents=True, exist_ok=True)
  base = f"{stem(cfg)}-windows-{NAMES[arch]}-setup"
  with tempfile.TemporaryDirectory() as tmp:
    iss = Path(tmp) / f"{cfg.name}.iss"
    text = script(cfg, binary.resolve(), arch, out_dir.resolve(), base, cfg.root / "LICENSE")
    # Inno reads UTF-8 with its byte-order mark: a name may not be ASCII.
    iss.write_text(text, encoding="utf-8-sig")
    run(iscc, "/Q", iss)

  return out_dir / f"{base}.exe"


def compiler() -> str:
  """ISCC: GitHub's Windows runners have it preinstalled (6.7.1 on windows-latest, image
  20261021); else on PATH; else exits naming how to install it."""
  where = Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)"))
  iscc = where / "Inno Setup 6" / "ISCC.exe"
  found = str(iscc) if iscc.is_file() else shutil.which("iscc")
  if not found:
    sys.exit(
      f"pack: no Inno Setup 6 at {iscc} or ISCC on PATH; install it: choco install innosetup"
    )

  return found


def app_id(cfg: Config) -> str:
  """The installer's AppId, the same in every version, so a newer setup replaces the install:
  `windows.app-id`, else a GUID made from the app's id. Never changed.

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


def script(cfg: Config, binary: Path, arch: str, out_dir: Path, base: str, license: Path) -> str:
  r"""The .iss for the installer `base` in `out_dir`.

  >>> from types import SimpleNamespace as N
  >>> cfg = N(id="me.vladkens.counter", name="Counter", version="1.2.0-3-gabc", comment="Counts",
  ...   windows={}, publisher="")
  >>> iss = script(cfg, Path("c.exe"), "aarch64", Path("out"), "b", Path("-"))
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


# MARK: Linux
# Ported from wavo-app's scripts/dist.py. The .desktop entry is named by the app id, nimbo's X11
# WM_CLASS and Wayland app id, so desktops find it, and its icon, from the window.

APPIMAGETOOL = (
  "https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-{arch}.AppImage"
)
RUNTIME = "https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-{arch}"
SHA256 = {
  "appimagetool-x86_64": "ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0",
  "appimagetool-aarch64": "f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158",
  "runtime-x86_64": "2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d",
  "runtime-aarch64": "00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444",
}
DEB_ARCH = {"x86_64": "amd64", "aarch64": "arm64"}
# The libraries nimbo loads at run time, by their Debian names (t64 since Ubuntu 24.04).
NIMBO = [
  "libgtk-3-0 | libgtk-3-0t64",
  "libgtk-layer-shell0",
  "libfontconfig1",
  "libfreetype6",
  "libharfbuzz0b",
  "libx11-6",
  "libxtst6",
]


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

  >>> deb_version("1.2.0-beta.1"), deb_version("1.2.0")
  ('1.2.0~beta.1', '1.2.0')
  """
  return version.replace("-", "~", 1)


def fetch(url: str, path: Path) -> Path:
  """`url` into `path`, once, checked against SHA256 by the file's name."""
  if not path.exists():
    path.parent.mkdir(parents=True, exist_ok=True)
    part = path.with_suffix(".part")
    with urllib.request.urlopen(url, timeout=60) as r, open(part, "wb") as f:
      shutil.copyfileobj(r, f)

    part.rename(path)

  digest = hashlib.sha256(path.read_bytes()).hexdigest()
  if digest != SHA256[path.name]:
    path.unlink()
    sys.exit(f"pack: {path.name}: SHA-256 {digest}, expected {SHA256[path.name]}")

  path.chmod(0o755)
  return path


def appimage(cfg: Config, binary: Path, arch: str, out_dir: Path) -> Path:
  """`<stem>-linux-<arch>.AppImage`: an AppDir of the binary, AppRun, the .desktop
  entry and the icon, packed by appimagetool with the static type2 runtime (no libfuse2), both
  pinned and fetched into <target dir>/appimage. nimbo links only glibc and loads the rest."""
  # The tool runs here; the runtime is the AppImage's own, of its arch.
  machine = host.machine().lower()
  here = {"amd64": "x86_64", "arm64": "aarch64"}.get(machine, machine)
  tools = cfg.target / "appimage"
  tool = fetch(APPIMAGETOOL.format(arch=here), tools / f"appimagetool-{here}")
  runtime = fetch(RUNTIME.format(arch=arch), tools / f"runtime-{arch}")
  out_dir.mkdir(parents=True, exist_ok=True)
  out = out_dir / f"{stem(cfg)}-linux-{arch}.AppImage"
  with tempfile.TemporaryDirectory() as tmp:
    appdir = Path(tmp) / f"{cfg.name}.AppDir"
    (appdir / "usr/bin").mkdir(parents=True)
    shutil.copy2(binary, appdir / "usr/bin" / cfg.bin)
    (appdir / "AppRun").symlink_to(f"usr/bin/{cfg.bin}")
    (appdir / f"{cfg.id}.desktop").write_text(desktop(cfg))
    shutil.copy2(cfg.icon, appdir / f"{cfg.id}.png")
    (appdir / ".DirIcon").symlink_to(f"{cfg.id}.png")
    # Runs without FUSE (CI has none): the tool unpacks itself.
    env = {**os.environ, "APPIMAGE_EXTRACT_AND_RUN": "1", "ARCH": arch}
    run(tool, "--no-appstream", "--runtime-file", runtime, appdir, out, env=env)

  return out


def put(path: Path, data: bytes) -> None:
  path.parent.mkdir(parents=True, exist_ok=True)
  path.write_bytes(data)


def deb(cfg: Config, binary: Path, arch: str, out_dir: Path) -> Path:
  """`<stem>-linux-<arch>.deb` (apt installs it by any name) by dpkg-deb: /usr/bin/<bin>, the .desktop entry, the
  icon in hicolor's 512x512 (hicolor has no 1024 folder; GTK scales it) and pixmaps, the LICENSE
  as the copyright. `linux.deb-package` names it (else the binary), `linux.deb-depends` and
  `deb-recommends` add to its relations."""
  author = next(iter(cfg.authors), "")
  if not re.search(r"<.+@.+>", author):
    sys.exit("pack: deb needs `authors` in Cargo.toml (its Maintainer)")

  pkg = cfg.linux.get("deb-package") or cfg.bin.lower().replace("_", "-")
  version = deb_version(cfg.version)
  out_dir.mkdir(parents=True, exist_ok=True)
  out = out_dir / f"{stem(cfg)}-linux-{arch}.deb"
  with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp) / pkg
    share = root / "usr/share"
    put(root / "usr/bin" / cfg.bin, binary.read_bytes())
    put(share / f"applications/{cfg.id}.desktop", desktop(cfg).encode())
    for dir in ("icons/hicolor/512x512/apps", "pixmaps"):
      put(share / dir / f"{cfg.id}.png", cfg.icon.read_bytes())

    license = cfg.root / "LICENSE"
    if license.is_file():
      put(share / f"doc/{pkg}/copyright", license.read_bytes())

    # dpkg-deb takes the modes as they are: 0755 for the directories and the binary.
    size = 0
    for dir, _, files in os.walk(root):
      os.chmod(dir, 0o755)
      for name in files:
        path = Path(dir) / name
        path.chmod(0o755 if path == root / "usr/bin" / cfg.bin else 0o644)
        size += path.stat().st_size

    control = {
      "Package": pkg,
      "Version": version,
      "Architecture": DEB_ARCH[arch],
      "Maintainer": author,
      "Installed-Size": str(size // 1024 + 1),
      "Depends": ", ".join(cfg.linux.get("deb-depends", [])),
      # What nimbo loads at run time (GTK and the text stack), then the app's own.
      "Recommends": ", ".join(dict.fromkeys(NIMBO + cfg.linux.get("deb-recommends", []))),
      "Section": "utils",
      "Priority": "optional",
      "Description": cfg.comment or cfg.name,
    }
    (root / "DEBIAN").mkdir(mode=0o755)
    (root / "DEBIAN/control").write_text("".join(f"{k}: {v}\n" for k, v in control.items() if v))
    # xz, not Ubuntu's default zstd, which older dpkg can't read.
    run("dpkg-deb", "-Zxz", "--root-owner-group", "--build", root, out)

  return out


# MARK: Feed

PLATFORMS = {f"{o}-{a}" for o in ("macos", "windows", "linux") for a in ("x86_64", "aarch64")}
# An Ed25519 private key's PKCS#8 DER (RFC 8410) before its 32-byte seed.
SEED_DER = bytes.fromhex("302e020100300506032b657004220420")
# Homebrew's OpenSSL 3, on Apple silicon and on Intel, when the PATH's is LibreSSL.
OPENSSL = (
  "openssl",
  "/opt/homebrew/opt/openssl@3/bin/openssl",
  "/usr/local/opt/openssl@3/bin/openssl",
)


def platform(name: str) -> str | None:
  """A file's feed platform by its name, None when the name doesn't say.

  >>> platform("Wavo-v1.2.0-linux-aarch64.AppImage"), platform("Wavo-v1.2.0-windows-x64-setup.exe")
  ('linux-aarch64', 'windows-x86_64')
  >>> platform("Wavo-v1.2.0-macos-aarch64.dmg"), platform("Wavo-v1.2.0-macos.zip")
  ('macos-aarch64', None)
  >>> platform("Wavo-dev-macos-aarch64.dmg"), platform("Wavo-dev-windows-arm64-setup.exe")
  ('macos-aarch64', 'windows-aarch64')
  >>> platform("Wavo-dev-linux-x86_64.AppImage"), platform("Wavo-dev-linux-aarch64.deb")
  ('linux-x86_64', 'linux-aarch64')
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


@functools.cache
def openssl3() -> str:
  """The first OpenSSL 3 or newer of OPENSSL; exits naming how to install it."""
  for tool in filter(None, map(shutil.which, OPENSSL)):
    out = subprocess.run([tool, "version"], capture_output=True, text=True).stdout
    m = re.match(r"OpenSSL (\d+)\.", out)
    if m and int(m[1]) >= 3:
      return tool

  sys.exit(
    "pack: Ed25519 needs OpenSSL 3, and macOS's openssl is LibreSSL: `brew install openssl@3`"
    f" puts it at {OPENSSL[1]} ({OPENSSL[2]} on Intel)"
  )


def openssl(*args, input: bytes | None = None) -> bytes:
  return run(openssl3(), *args, input=input, capture_output=True).stdout


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
  # Signed beside it, then in place: a failed signature leaves the feed that was there.
  path, part = out / "update.txt", out / "update.txt.part"
  part.write_bytes(body(version, packages))
  # Ed25519 signs only a file: the feed is one, the key comes on stdin.
  sig = openssl("pkeyutl", "-sign", "-inkey", "/dev/stdin", "-rawin", "-in", part, input=key)
  with part.open("ab") as f:
    f.write(f"sig {sig.hex()}\n".encode())

  part.replace(path)

  print(f"{path}: {version} for {', '.join(p for p, _, _ in packages)}, signed")
  return path


def feed_key(repo: str | None) -> None:
  # Never a second key: every app shipped trusts the first one's public half only.
  where = ["--repo", repo] if repo else []
  names = run("gh", "secret", "list", *where, capture_output=True, text=True).stdout.split()
  if "UPDATE_KEY" in names:
    sys.exit("pack: UPDATE_KEY is set already; the apps shipped trust it: delete it by hand first")

  if not shutil.which("pbcopy"):
    sys.exit("pack: feed-key runs on a Mac: pbcopy hands the private key over")

  key = openssl("genpkey", "-algorithm", "ed25519")
  seed = openssl("pkey", "-outform", "DER", input=key)[-32:].hex()
  # The clipboard first: a key only in the secret could never be read back.
  run("pbcopy", input=seed.encode())
  run("gh", "secret", "set", "UPDATE_KEY", *where, input=seed.encode())
  print(
    "pack: the private key is UPDATE_KEY and on the clipboard: keep it in Bitwarden",
    file=sys.stderr,
  )
  print(public_key(pem(seed)))


# MARK: Command line


def main():
  ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
  ap.add_argument("command", choices=("dist", *OS, "feed", "feed-key"))
  ap.add_argument("files", nargs="*", help="dmg: the .app (or --app); feed: [<platform>=]<file>")
  ap.add_argument("--manifest", type=Path, default=Path("Cargo.toml"), help="the app's Cargo.toml")
  ap.add_argument("--target", help="the target triple, by default this machine's arch")
  ap.add_argument(
    "--bin", type=Path, help="the binary, by default <target dir>/<triple>/dist/<bin>"
  )
  ap.add_argument("--app", type=Path, help="dmg: the .app, by default <target dir>/<Name>.app")
  ap.add_argument("--label", help="the files' <Name>-<label>-…, by default <Name>-v<version>-…")
  ap.add_argument("--out", type=Path, help="where the files go, by default <target dir>/distrib")
  ap.add_argument("--key", type=Path, help="feed: the private key's file, else $UPDATE_KEY")
  ap.add_argument("--url", help="feed: where the files will be served, {version} the version")
  ap.add_argument("--public-key", action="store_true", help="feed: print the key's public half")
  ap.add_argument("--repo", help="feed-key: the secret's GitHub repo, by default the current one")
  args = ap.parse_intermixed_args()
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

  if args.label and not re.fullmatch(r"[A-Za-z0-9.-]+", args.label):
    ap.error("--label is letters, digits, dots and hyphens")

  cfg = load(args.manifest)
  cfg.label = args.label
  out = args.out or cfg.target / "distrib"
  if args.command == "feed":
    feed(pem(key), cfg.version, args.url, args.files, out)
    return

  if args.command == "dmg":
    bundles = [*args.files, *([args.app] if args.app else [])]
    if len(bundles) > 1:
      ap.error("dmg takes one .app")

    dmg(cfg, Path(bundles[0]) if bundles else cfg.target / f"{cfg.name}.app", args.target, out)
    return

  if args.files or args.app:
    ap.error(f"{args.command} takes no .app or files")

  # The .app goes beside the files, never among them.
  if args.command == "app" and args.out:
    ap.error("app writes <target dir>/<Name>.app: --out is the files'")

  steps = DIST.get(sys.platform) if args.command == "dist" else (args.command,)
  if not steps:
    sys.exit(f"pack: dist runs on macOS, Windows and Linux, not {sys.platform}")

  target = args.target or f"{arch(host.machine())}-{OS[steps[0]]}"
  exe = cfg.bin + (".exe" if "windows" in target else "")
  binary = args.bin or cfg.target / target / "dist" / exe
  if not binary.is_file():
    sys.exit(f"pack: no binary at {binary}: build it, or give --bin")

  for step in steps:
    if step == "app":
      bundle = app(cfg, binary)
    elif step == "dmg":
      dmg(cfg, bundle, args.target, out)
    else:
      make = {"setup": setup, "appimage": appimage, "deb": deb}[step]
      print(make(cfg, binary, arch(target), out))


if __name__ == "__main__":
  main()
