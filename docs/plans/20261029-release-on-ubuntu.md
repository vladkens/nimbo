# Release on Ubuntu

Goal: an app's release files (the macOS `.app` and DMG, the Windows setup, the AppImage and `.deb`, the feed) all made and signed in one Ubuntu publish job of vladkens/actions' rust-publish, from the bare binaries the build jobs leave, with no macOS or Windows runner in the publish step. Agreed with the person 2026-10-29, from the draft of the same day.

## Decisions

- `scripts/pack.py` stays one file: wavo-app fetches it as one. Plain `python3` 3.9+ with the standard library, doctested under `python3` and `/usr/bin/python3`.
- The build jobs only run `cargo build` and copy the binary to `target/distrib/<triple>/<bin>[.exe]`; pack.py isn't used there.
- The Windows exe's icon, version and manifest are written by pack.py's own PE resource code in the Ubuntu publish job: no rcedit, no npm, no build.rs.
- No Wine anywhere.
- sips, iconutil, vtool and lipo give way to pure Python on every OS: a PNG codec, the `.icns` and `.ico` writers and a Mach-O reader.
- The `.icns` writes 16 and 32 px as `ic04` and `ic05` (ARGB), as iconutil does, not as PNG in `icp4` and `icp5`: macOS 27's iconutil reads those as noise (checked 2026-10-29).

## Stages

- [x] 1. In pure Python: PNG decode and encode, the alpha-weighted resize and the tile crop (from wavo-app's build.rs), the `.icns` (ic04 ic05 ic07–ic14) and `.ico` (16–256) writers, a Mach-O reader (the arch, thin and fat, and the minimum macOS), and `exe_resources`, which writes the exe's RT_GROUP_ICON and RT_ICON, RT_VERSION and RT_MANIFEST into its `.rsrc`, merged with the linker's. Wired into today's `app`, `dmg` and `setup`. Checked on the `counter` example cross-built for x64 and arm64 Windows.
  - Result: `scripts/pack.py` 945 → 1,680 lines (the images ~310, Mach-O ~70, the exe's resources ~360), doctested under `python3` 3.13 and `/usr/bin/python3` 3.9. The PNG decoder matches Pillow on 45 files using all five filters and every supported colour type; the `.icns` round-trips through iconutil; the Mach-O reader matches lipo and vtool on thin and fat binaries. `exe_resources` on `counter.exe` (cargo xwin, x64 and arm64, and x64 with lld-link's embedded manifest, `.rsrc` before `.reloc`): llvm-readobj and pefile read the icon group (6 PNG icons), the version, the manifest (ours replacing lld-link's), the same imports and base relocations, a correct checksum, and a second run gives the same bytes. Not yet run on real Windows.
- [ ] 2. macOS on Linux: `app` signed by rcodesign, the DMG by `mkfs.hfsplus`, libdmg-hfsplus's `hfsplus` and `dmg`, then rcodesign's notarization and staple; codesign and hdiutil stay for local Mac builds.
- [ ] 3. NSIS in place of Inno Setup, built by `makensis` on Ubuntu, and update.rs's Windows setup run as `/S`.
- [ ] 4. `release <dir> --label <dev|vX.Y.Z> [--tag vX.Y.Z]`: reads `<dir>/<triple>/<bin>[.exe]` and writes the release files at `<dir>`'s top level, plus update.txt only with a tag, its URL `<Cargo.toml repository>/releases/download/<tag>/`.
