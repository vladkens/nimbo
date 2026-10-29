# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
"""Makes the macOS app bundle, signed, and the DMG that holds it.

`app`: `<Name>.app` with the binary, `<Name>.icns` (every size of an iconset scaled down from the
1024 px PNG) and an Info.plist from the config: the bundle's id, name and version, the least
macOS the binary is built for (vtool, as MACOSX_DEPLOYMENT_TARGET set it), `LSUIElement` when
`macos.accessory`, then `macos.usage` (the permission prompts' strings) and `macos.plist` over it.

It is signed with the hardened runtime (which notarization needs) and `macos.entitlements` when
set, by: $NIMBO_SIGN_IDENTITY (`-` ad hoc), else a Developer ID in the keychain, else ad hoc in CI,
else the app's own identity, "<Name> Local Signing", made once in the login keychain: its
designated requirement names the bundle id and the certificate, not the code hash, so the
permissions macOS grants survive rebuilds; the certificate stays untrusted, which that doesn't
need. A bundle that can't be signed (codesign failed or didn't finish in time: a keychain dialog
nobody answers) is no bundle, never one signed ad hoc, whose grants macOS would keep for that build
only. The release's signing, notarization and stapling are vladkens/actions' macos-sign.

`dmg`: `<Name>-v<version>-macos.dmg`, compressed (UDZO), of the `.app` and a link to /Applications.
"""

from __future__ import annotations

import os
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from pack import Config, run

KEYS = {"accessory", "usage", "plist", "entitlements"}


def info(cfg: Config, minos: str) -> dict:
  """The Info.plist: the bundle from the config, then its `usage` strings and `plist` keys over it.

  >>> mac = {"accessory": True, "usage": {"NSMicrophoneUsageDescription": "m"}, "plist": {"X": 1}}
  >>> p = info(Config("me.a", "A", Path("i.png"), "", "1.2.0-beta.1", "a", mac, {}, Path(".")), "14.0")
  >>> p["CFBundleExecutable"], p["CFBundleVersion"], p["LSUIElement"], p["X"], p["LSMinimumSystemVersion"]
  ('a', '1.2.0', True, 1, '14.0')
  """
  bad = set(cfg.macos) - KEYS
  if bad:
    sys.exit(f"pack: unknown [package.metadata.nimbo.macos] keys {', '.join(sorted(bad))}: one of {', '.join(sorted(KEYS))}")

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
        run("sips", "-z", px, px, png, "--out", Path(tmp) / f"icon_{size}x{size}{suffix}.png", capture_output=True)

    run("iconutil", "-c", "icns", "-o", out, tmp)


def identities(valid: bool) -> list[str]:
  out = run("security", "find-identity", *(["-v"] if valid else []), "-p", "codesigning", capture_output=True, text=True)
  return [ln for ln in out.stdout.splitlines() if '"' in ln]


def make_identity(name: str) -> None:
  """A self-signed code signing certificate in the login keychain, codesign allowed to use it."""
  with tempfile.TemporaryDirectory() as tmp:
    key, cert, p12 = (Path(tmp) / n for n in ("key.pem", "cert.pem", "identity.p12"))
    # macOS's own openssl (LibreSSL): `security import` reads its PKCS#12, not OpenSSL 3's.
    openssl = "/usr/bin/openssl"
    run(openssl, "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "3650", "-subj", f"/CN={name}",
        "-addext", "keyUsage=critical,digitalSignature", "-addext", "extendedKeyUsage=critical,codeSigning",
        "-keyout", key, "-out", cert, capture_output=True)
    run(openssl, "pkcs12", "-export", "-inkey", key, "-in", cert, "-out", p12, "-passout", "pass:nimbo", capture_output=True)
    keychain = Path.home() / "Library/Keychains/login.keychain-db"
    run("security", "import", p12, "-k", keychain, "-P", "nimbo", "-T", "/usr/bin/codesign", capture_output=True)

  if not any(f'"{name}"' in ln for ln in identities(False)):
    sys.exit(f'pack: made "{name}", but `security find-identity` does not list it')

  print(f'pack: made the code signing identity "{name}" in the login keychain', file=sys.stderr)


def identity(cfg: Config) -> tuple[str, str]:
  """What signs the app, as codesign takes it and as it is said."""
  name = os.environ.get("NIMBO_SIGN_IDENTITY")
  if name:
    return name, "ad hoc" if name == "-" else name

  # A Developer ID by its hash, as two (a renewal) would be ambiguous by name.
  dev = [ln for ln in identities(True) if '"Developer ID Application:' in ln]
  if dev:
    return dev[0].split()[1], dev[0].split('"')[1]

  if os.environ.get("CI"):
    return "-", "ad hoc (CI)"

  name = f"{cfg.name} Local Signing"
  if not any(f'"{name}"' in ln for ln in identities(False)):
    make_identity(name)

  return name, name


def app(cfg: Config, binary: Path, out_dir: Path) -> Path:
  sha, name = identity(cfg)
  bundle = out_dir / f"{cfg.name}.app"
  shutil.rmtree(bundle, ignore_errors=True)
  (bundle / "Contents/MacOS").mkdir(parents=True)
  (bundle / "Contents/Resources").mkdir(parents=True)
  shutil.copy2(binary, bundle / "Contents/MacOS" / cfg.bin)
  icns(cfg.icon, bundle / f"Contents/Resources/{cfg.name}.icns")
  (bundle / "Contents/PkgInfo").write_text("APPL????")
  plist = bundle / "Contents/Info.plist"
  with plist.open("wb") as f:
    plistlib.dump(info(cfg, minos(binary)), f)

  run("plutil", "-lint", "-s", plist)
  run("xattr", "-cr", bundle)
  entitlements = cfg.macos.get("entitlements")
  extra = ["--entitlements", cfg.root / entitlements] if entitlements else []
  try:
    run("codesign", "--force", "--sign", sha, "--options", "runtime", *extra, "--timestamp=none", bundle, timeout=60)
  except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as e:
    shutil.rmtree(bundle, ignore_errors=True)
    sys.exit(f'pack: codesign with "{name}" failed ({e}): no bundle')

  run("codesign", "--verify", "--strict", bundle)
  print(f"{bundle}: {cfg.name} {cfg.version}, signed: {name}")
  return bundle


def dmg(cfg: Config, app: Path, out_dir: Path) -> Path:
  if not (app / "Contents/Info.plist").is_file():
    sys.exit(f"pack: {app} is no app bundle")

  out = out_dir / f"{cfg.name}-v{cfg.version}-macos.dmg"
  out_dir.mkdir(parents=True, exist_ok=True)
  with tempfile.TemporaryDirectory() as tmp:
    stage = Path(tmp) / cfg.name
    stage.mkdir()
    # ditto keeps the signature, the links and the modes as they are.
    run("ditto", app, stage / app.name)
    (stage / "Applications").symlink_to("/Applications")
    # hdiutil, which macOS 27 deprecates for `diskutil image`: the runners up to macOS 15 lack that.
    run("hdiutil", "create", "-volname", cfg.name, "-srcfolder", stage, "-format", "UDZO", "-ov", out, capture_output=True)

  print(f"{out}: {cfg.name} {cfg.version}")
  return out
