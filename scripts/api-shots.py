# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
# /// script
# requires-python = ">=3.11"
# ///
"""Makes docs/api.md and its pictures from the gallery's table (examples/gallery.rs).

The gallery prints the page (`--api`) and the blocks' slugs (`--list`); each picture is a block
alone, as high as its variants, in the light appearance at 2x, from the harness without a window
(`<slug> --shot target/api-shots/<slug>.png`), compressed as TinyPNG would (pngquant, then
oxipng: `brew install pngquant oxipng`). The pictures live on the repo's `assets` branch:
when its worktree is there (`<root>/.worktrees/assets`) they go to its `api/`, those of blocks no
longer in the table out of it, and the gallery's test checks every one byte for byte
(`NIMBO_API_SHOTS_STRICT`); a person commits and pushes that branch after looking at them.

uv run scripts/api-shots.py
"""

import os
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "target" / "api-shots"


def assets() -> Path:
  """The `assets` branch's worktree: `.worktrees/assets` of the repo's main checkout."""
  common = ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"]
  git = subprocess.run(common, cwd=ROOT, check=True, capture_output=True, text=True)
  return Path(git.stdout.strip()).parent / ".worktrees" / "assets"


QUANT = ["pngquant", "--quality=80-95", "--strip", "--skip-if-larger", "--force", "--ext", ".png"]
OXI = ["oxipng", "-o", "max", "--strip", "all"]


def compress(pngs: list[Path]) -> None:
  """Each of `pngs` quantized (kept as it was where that is no smaller), then losslessly packed."""
  for tool in (QUANT[0], OXI[0]):
    if shutil.which(tool) is None:
      raise SystemExit(f"api-shots: no {tool}; brew install pngquant oxipng")

  for png in pngs:
    # 98 and 99: pngquant kept the picture, smaller or good enough as it was.
    if subprocess.run([*QUANT, png], capture_output=True).returncode not in (0, 98, 99):
      raise SystemExit(f"api-shots: pngquant failed on {png}")

  subprocess.run([*OXI, *pngs], check=True, capture_output=True)


def main() -> None:
  subprocess.run(["cargo", "build", "--release", "--example", "gallery"], cwd=ROOT, check=True)
  gallery = ROOT / "target" / "release" / "examples" / "gallery"
  run = lambda *a: subprocess.run([gallery, *a], cwd=ROOT, check=True, capture_output=True, text=True)
  (ROOT / "docs" / "api.md").write_text(run("--api").stdout)
  slugs = run("--list").stdout.split()
  OUT.mkdir(parents=True, exist_ok=True)
  for slug in slugs:
    run(slug, "--shot", str(OUT / f"{slug}.png"), "--appearance", "light")

  for old in OUT.glob("*.png"):
    if old.stem not in slugs:
      old.unlink()

  pngs = [OUT / f"{slug}.png" for slug in slugs]
  raw = sum(p.stat().st_size for p in pngs)
  compress(pngs)
  print(f"{len(pngs)} pictures: {raw / 1e6:.2f} MB, compressed {sum(p.stat().st_size for p in pngs) / 1e6:.2f} MB")
  branch = assets()
  if not branch.is_dir():
    print(f"docs/api.md, and {len(slugs)} pictures in {OUT} (no {branch} to copy them to)")
    return

  api = branch / "api"
  api.mkdir(exist_ok=True)
  for old in api.glob("*.png"):
    if old.stem not in slugs:
      old.unlink()

  for slug in slugs:
    shutil.copyfile(OUT / f"{slug}.png", api / f"{slug}.png")

  # The gallery's test, strict: each picture byte for byte as the gallery draws it now.
  env = {**os.environ, "NIMBO_API_SHOTS_STRICT": "1"}
  test = ["cargo", "test", "--test", "gallery", "the_api_doc_is_the_gallerys"]
  subprocess.run(test, cwd=ROOT, env=env, check=True, capture_output=True)
  print(f"docs/api.md, and {len(slugs)} pictures in {api}: look at them, then commit and push")


if __name__ == "__main__":
  main()
