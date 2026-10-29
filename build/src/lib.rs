// Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo

//! The build script of a nimbo app: its icons and, on Windows, the exe's resources, from the
//! package's `[package.metadata.nimbo]` (the config `scripts/pack.py` reads).
//!
//! `nimbo_build::app()` in `build.rs` does it all ([app]); the pieces are here for an app that
//! wants only part: [window_icon], [rgba], [windows_resources] with a [Meta] of its own.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

/// The sizes in the exe's icon: Explorer's views and the taskbar at 100–200% scale.
const ICO: [usize; 6] = [16, 24, 32, 48, 64, 256];

/// What the resources say of the app: the config's `id`, `name`, `icon` and `publisher`, and the
/// package's version.
#[derive(Clone, Debug, PartialEq)]
pub struct Meta {
  pub id: String,
  pub name: String,
  /// The PNG, 1024 px as pack.py takes it, the path absolute.
  pub icon: PathBuf,
  /// The config's, else the first of the package's authors, else the id's owner (`me.vladkens.x`)
  /// as pack.py's installer has it.
  pub publisher: String,
  pub version: String,
}

impl Meta {
  /// The package's: its `Cargo.toml` and Cargo's `CARGO_PKG_*`.
  pub fn load() -> Self {
    let dir = PathBuf::from(env("CARGO_MANIFEST_DIR"));
    let manifest = dir.join("Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("read Cargo.toml: {e}"));
    Self::parse(&text, &dir, &env("CARGO_PKG_VERSION"), &env("CARGO_PKG_AUTHORS"))
  }

  /// From a manifest's `text`, in `dir`; `authors` separated by `:`, as Cargo gives them.
  fn parse(text: &str, dir: &Path, version: &str, authors: &str) -> Self {
    let toml: toml::Table = text.parse().unwrap_or_else(|e| panic!("Cargo.toml: {e}"));
    let cfg = toml.get("package").and_then(|p| p.get("metadata")?.get("nimbo"));
    let key = |k: &str| cfg.and_then(|c| c.get(k)?.as_str()).map(str::to_owned);
    let need = |k: &str| key(k).unwrap_or_else(|| panic!("no `{k}` in [package.metadata.nimbo]"));
    let (id, name, icon) = (need("id"), need("name"), dir.join(need("icon")));
    let author =
      authors.split(':').next().and_then(|a| a.split(" <").next()).filter(|a| !a.is_empty());
    let owner = || id.split('.').nth(if id.matches('.').count() > 1 { 1 } else { 0 });
    let publisher = key("publisher").or(author.map(str::to_owned)).or(owner().map(str::to_owned));
    Self { publisher: publisher.unwrap_or_default(), id, name, icon, version: version.to_owned() }
  }

  /// The version as Windows has it, numbers only: `1.2.0-3-gabc` is `[1, 2, 0, 0]`.
  fn numbers(&self) -> [u16; 4] {
    let plain = self.version.split(['-', '+']).next().unwrap_or_default();
    let mut out = [0; 4];
    for (n, part) in out.iter_mut().zip(plain.split('.')) {
      *n = part.parse().unwrap_or(0);
    }

    out
  }
}

/// All of it, from the package's config: `window-icon.rgba` in `$OUT_DIR` ([window_icon], 256
/// px) and, building for Windows, the exe's icon, version and manifest ([windows_resources]).
pub fn app() {
  let meta = Meta::load();
  window_icon(&meta.icon, 256);
  windows_resources(&meta);
}

/// `$OUT_DIR/window-icon.rgba`, for `nimbo::Config::icon`: the PNG at `png` [cropped] to its tile,
/// `side` × `side` straight RGBA.
pub fn window_icon(png: impl AsRef<Path>, side: usize) {
  let (n, tile) = cropped(&decode(png.as_ref()));
  write(&out_dir().join("window-icon.rgba"), &resized(n, n, &tile, side));
}

/// `$OUT_DIR/<file>`: the PNG at `png` as a whole, margin and all, `side` × `side` straight RGBA,
/// as `nimbo::Ui::rgba_image` takes it.
pub fn rgba(png: impl AsRef<Path>, side: usize, file: &str) {
  let (w, h, px) = decode(png.as_ref());
  write(&out_dir().join(file), &resized(w, h, &px, side));
}

/// Building for Windows, a resource linked into the binaries: the icon Explorer shows (its tile
/// at [ICO]'s sizes), the version Properties and Task Manager show, and the manifest. Nothing for
/// another target. Compiled by rc.exe, or llvm-rc when cross-compiling (on the `PATH`).
pub fn windows_resources(meta: &Meta) {
  if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
    return;
  }

  let out = out_dir();
  let (n, tile) = cropped(&decode(&meta.icon));
  let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
  for side in ICO {
    let image =
      ico::IconImage::from_rgba_data(side as u32, side as u32, resized(n, n, &tile, side));
    dir.add_entry(ico::IconDirEntry::encode(&image).expect("an icon image"));
  }

  let ico = out.join("app.ico");
  dir.write(File::create(&ico).expect("app.ico")).expect("write app.ico");
  let manifest = out.join("app.manifest");
  write(&manifest, MANIFEST.as_bytes());
  let rc = out.join("app.rc");
  write(&rc, script(meta, &ico, &manifest).as_bytes());

  // Without a resource compiler a debug build (a check of another target) has none, said as a
  // warning; a release build fails rather than ship an exe without them.
  let res = embed_resource::compile(&rc, embed_resource::NONE);
  if let embed_resource::CompilationResult::NotAttempted(why) = &res {
    assert!(env("PROFILE") != "release", "the exe's resources need rc.exe or llvm-rc: {why}");
    println!("cargo:warning=no exe resources, no rc.exe or llvm-rc: {why}");
  }

  res.manifest_optional().expect("app.rc");
}

/// The resource script: the icon first, which Windows takes for the exe's, VERSIONINFO and the
/// manifest. UTF-8, as rc.exe reads the ANSI code page otherwise (a path with an accented user
/// name, a `©`); numbers for winver.h's names, as llvm-rc doesn't preprocess.
fn script(meta: &Meta, ico: &Path, manifest: &Path) -> String {
  let path = |p: &Path| p.display().to_string().replace('\\', "/");
  let s = |v: &str| v.replace('"', "\"\"");
  let [a, b, c, d] = meta.numbers();
  let (version, name, publisher) = (format!("{a}.{b}.{c}"), s(&meta.name), s(&meta.publisher));
  let values = [
    ("CompanyName", publisher.clone()),
    ("FileDescription", name.clone()),
    ("FileVersion", version.clone()),
    ("LegalCopyright", format!("© {publisher}")),
    ("OriginalFilename", format!("{name}.exe")),
    ("ProductName", name),
    ("ProductVersion", version),
  ];

  let values: String =
    values.iter().map(|(k, v)| format!("      VALUE \"{k}\", \"{v}\"\n")).collect();
  let (ico, manifest) = (path(ico), path(manifest));
  // FILEOS VOS_NT_WINDOWS32, FILETYPE VFT_APP; 0x409 1200 US English in UTF-16. RT_MANIFEST 24.
  format!(
    r#"#pragma code_page(65001)
1 ICON "{ico}"
1 VERSIONINFO
FILEVERSION {a},{b},{c},{d}
PRODUCTVERSION {a},{b},{c},{d}
FILEOS 0x40004
FILETYPE 1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
{values}    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
1 24 "{manifest}"
"#
  )
}

/// The exe's manifest. `asInvoker`: no UAC prompt, even for a name like `setup` or `update` that
/// Windows' installer detection would elevate. Windows 10 and 11, whose `supportedOS` id is one.
/// Per-monitor DPI v2, as nimbo's `SetProcessDpiAwarenessContext` asks (which then fails as the
/// manifest set it already: the same awareness). Common Controls v6, the themed dialogs.
const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges>
    <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
  </requestedPrivileges></security></trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1"><application>
    <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
  </application></compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3"><windowsSettings>
    <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
    <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
  </windowsSettings></application>
  <dependency><dependentAssembly>
    <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
      processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
  </dependentAssembly></dependency>
</assembly>
"#;

fn env(key: &str) -> String {
  std::env::var(key).unwrap_or_else(|_| panic!("{key}: not in a build script"))
}

fn out_dir() -> PathBuf {
  PathBuf::from(env("OUT_DIR"))
}

fn write(path: &Path, data: &[u8]) {
  fs::write(path, data).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

/// The PNG at `path` as `(width, height, RGBA)`, 8 bits a channel, not premultiplied.
fn decode(path: &Path) -> (usize, usize, Vec<u8>) {
  println!("cargo:rerun-if-changed={}", path.display());
  let file = File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
  let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
  decoder.set_transformations(png::Transformations::STRIP_16 | png::Transformations::ALPHA);
  let mut reader = decoder.read_info().expect("a PNG");
  let mut px = vec![0; reader.output_buffer_size().expect("its size")];
  let info = reader.next_frame(&mut px).expect("its pixels");
  assert_eq!(info.color_type, png::ColorType::Rgba, "{}: not RGB(A)", path.display());
  px.truncate(info.buffer_size());
  (info.width as usize, info.height as usize, px)
}

/// The image cut to the square around its visible pixels, centred: its side and pixels. macOS
/// icons keep a transparent margin around their tile (about a tenth on each side), Windows'
/// taskbar and Linux docks show the image as it is, the tile small.
fn cropped((w, h, px): &(usize, usize, Vec<u8>)) -> (usize, Vec<u8>) {
  let (w, h) = (*w, *h);
  let visible = |x: usize, y: usize| px[(y * w + x) * 4 + 3] > 8;
  let xs: Vec<usize> = (0..w).filter(|&x| (0..h).any(|y| visible(x, y))).collect();
  let ys: Vec<usize> = (0..h).filter(|&y| (0..w).any(|x| visible(x, y))).collect();
  let (Some(&x0), Some(&x1), Some(&y0), Some(&y1)) = (xs.first(), xs.last(), ys.first(), ys.last())
  else {
    return (w.max(h), resized(w, h, px, w.max(h)));
  };

  let side = (x1 - x0).max(y1 - y0) + 1;
  // The square's corner, the tile centred in it; outside the image is transparent.
  let left = (x0 + x1 + 1) as isize / 2 - side as isize / 2;
  let top = (y0 + y1 + 1) as isize / 2 - side as isize / 2;
  let mut tile = vec![0; side * side * 4];
  for y in 0..side {
    for x in 0..side {
      let (sx, sy) = (left + x as isize, top + y as isize);
      if sx < 0 || sy < 0 || sx >= w as isize || sy >= h as isize {
        continue;
      }

      let from = (sy as usize * w + sx as usize) * 4;
      tile[(y * side + x) * 4..][..4].copy_from_slice(&px[from..from + 4]);
    }
  }

  (side, tile)
}

/// `n` × `n` pixels, each the average of the source pixels it covers (at least the one under it,
/// scaling up), weighted by their alpha: averaging straight colours would bleed the transparent
/// black.
fn resized(w: usize, h: usize, px: &[u8], n: usize) -> Vec<u8> {
  let span = |i: usize, m: usize| i * m / n..((i + 1) * m / n).max(i * m / n + 1);
  let mut out = Vec::with_capacity(n * n * 4);
  for (y, x) in (0..n).flat_map(|y| (0..n).map(move |x| (y, x))) {
    let mut s = [0f64; 5];
    for (sy, sx) in span(y, h).flat_map(|sy| span(x, w).map(move |sx| (sy, sx))) {
      let p = &px[(sy * w + sx) * 4..][..4];
      let a = f64::from(p[3]);
      (0..3).for_each(|c| s[c] += f64::from(p[c]) * a);
      s[3] += a;
      s[4] += 1.0;
    }

    let color = |c: f64| if s[3] > 0.0 { (c / s[3]).round() as u8 } else { 0 };
    out.extend([color(s[0]), color(s[1]), color(s[2]), (s[3] / s[4]).round() as u8]);
  }

  out
}

#[cfg(test)]
mod tests {
  use super::*;

  const TOML: &str = "[package]\nname = \"a\"\n[package.metadata.nimbo]\nid = \"me.vladkens.a\"\n\
    name = \"A \\\"b\\\"\"\nicon = \"assets/icon.png\"\n";

  #[test]
  fn meta() {
    let m = Meta::parse(TOML, Path::new("/x"), "1.2.3-4-gabc", "Vlad <v@x.y>:B");
    assert_eq!(
      (m.id.as_str(), m.name.as_str(), m.publisher.as_str()),
      ("me.vladkens.a", "A \"b\"", "Vlad")
    );
    assert_eq!((m.numbers(), m.icon), ([1, 2, 3, 0], PathBuf::from("/x/assets/icon.png")));
    let m = Meta::parse(TOML, Path::new("/x"), "7.8", "");
    assert_eq!((m.publisher.as_str(), m.numbers()), ("vladkens", [7, 8, 0, 0]));
    let given =
      TOML.replace("[package.metadata.nimbo]\n", "[package.metadata.nimbo]\npublisher = \"P\"\n");
    assert_eq!(Meta::parse(&given, Path::new("/x"), "1.0.0", "B").publisher, "P");
  }

  #[test]
  fn script_quotes_and_numbers() {
    let m = Meta::parse(TOML, Path::new("/x"), "1.2.3-dev", "");
    let rc = script(&m, Path::new(r"C:\a\app.ico"), Path::new("/o/app.manifest"));
    assert!(rc.contains("1 ICON \"C:/a/app.ico\"\n1 VERSIONINFO\nFILEVERSION 1,2,3,0\n"));
    assert!(rc.contains("VALUE \"ProductName\", \"A \"\"b\"\"\"\n"));
    assert!(rc.contains("VALUE \"ProductVersion\", \"1.2.3\"\n"));
    assert!(rc.ends_with("1 24 \"/o/app.manifest\"\n"));
  }

  #[test]
  fn crop_centres_the_tile() {
    // 4 × 4, an opaque 2 × 2 tile at (1, 1).
    let mut px = vec![0; 64];
    for i in [5, 6, 9, 10] {
      px[i * 4..][..4].copy_from_slice(&[200, 100, 50, 255]);
    }

    let (side, tile) = cropped(&(4, 4, px));
    assert_eq!((side, tile.len()), (2, 16));
    assert!(tile.chunks(4).all(|p| p == [200, 100, 50, 255]));
    assert_eq!(resized(2, 2, &tile, 1), [200, 100, 50, 255]);
    assert_eq!(resized(2, 2, &tile, 3), [200, 100, 50, 255].repeat(9));
    assert_eq!(resized(4, 4, &[[0; 4], [9, 9, 9, 255]].concat().repeat(8), 2)[..4], [9, 9, 9, 128]);
  }
}
