# Copyright (c) vladkens | MIT License | https://github.com/vladkens/nimbo
# /// script
# requires-python = ">=3.11"
# ///
"""Makes docs/api.md and its pictures from the gallery's table (examples/gallery.rs).

The gallery prints the page (`--api`) and the blocks' slugs (`--list`); each picture is a block
alone, as high as its variants, in the light appearance at 2x, from the harness without a window
(`<slug> --shot docs/api/<slug>.png`). Pictures of blocks no longer in the table go.

uv run scripts/api-shots.py
"""

import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "docs" / "api"


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

  print(f"docs/api.md and {len(slugs)} pictures in docs/api/")


if __name__ == "__main__":
  main()
