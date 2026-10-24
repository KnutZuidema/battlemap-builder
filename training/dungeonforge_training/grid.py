"""Sample grid stitcher for checkpoint validation preview images.

Per ADR-0008 and ADR-0009, training evaluates validation prompts at each
250-step checkpoint. This module composes the preview images into a single
standardized sample grid image (samples.png) for publication and operator inspection.
"""

import math
from pathlib import Path
import re
from typing import Optional, Sequence

from PIL import Image


def stitch_sample_grid(
    image_paths: Sequence[Path | str],
    output_path: Path | str,
    max_cols: int = 2,
) -> Path:
    """Stitch multiple prompt sample images into a single grid image.

    For 1 sample: copies/saves directly as PNG.
    For 2 samples: 2x1 grid.
    For 4 samples: 2x2 grid (standard for DEFAULT_SAMPLE_PROMPTS).
    For N samples: grid layout with at most max_cols columns.
    """
    if not image_paths:
        raise ValueError("Cannot stitch an empty list of image paths")

    out_p = Path(output_path)
    out_p.parent.mkdir(parents=True, exist_ok=True)

    loaded_images: list[Image.Image] = []
    try:
        for p in image_paths:
            img = Image.open(p).convert("RGB")
            loaded_images.append(img)

        n = len(loaded_images)
        if n == 1:
            loaded_images[0].save(out_p, format="PNG")
            return out_p

        # Standard grid calculations
        if n == 2:
            cols = 2
            rows = 1
        elif n <= 4:
            cols = 2
            rows = math.ceil(n / 2)
        else:
            cols = min(n, max_cols)
            rows = math.ceil(n / cols)

        tile_w = max(img.width for img in loaded_images)
        tile_h = max(img.height for img in loaded_images)

        grid_img = Image.new("RGB", (cols * tile_w, rows * tile_h), color=(0, 0, 0))

        for idx, img in enumerate(loaded_images):
            c = idx % cols
            r = idx // cols
            x = c * tile_w
            y = r * tile_h
            if img.size != (tile_w, tile_h):
                img = img.resize((tile_w, tile_h), Image.Resampling.LANCZOS)
            grid_img.paste(img, (x, y))

        grid_img.save(out_p, format="PNG")
        return out_p
    finally:
        for img in loaded_images:
            try:
                img.close()
            except Exception:
                pass


def find_step_sample_images(samples_dir: Path | str, step: int) -> list[Path]:
    """Find all sample preview images belonging to a given training step.

    Matches patterns like:
      - {name}_{prompt_idx}_{step}.png
      - {name}_{step}_{prompt_idx}.png
      - step-{step}/*.png
      - sample_{step}_*.png
    """
    sdir = Path(samples_dir)
    if not sdir.exists():
        return []

    # Check step subdirectory first (e.g. samples_dir / f"step-{step}")
    step_subdir = sdir / f"step-{step}"
    if step_subdir.is_dir():
        imgs = sorted(
            [p for p in step_subdir.iterdir() if p.suffix.lower() in (".png", ".jpg", ".jpeg", ".webp")]
        )
        if imgs:
            return imgs

    # Search files in samples_dir matching step token
    step_str = str(step)
    step_patterns = [
        re.compile(rf"(^|[_\-\. ]){step_str}([_\-\. ]|$|\.png$|\.jpg$)", re.IGNORECASE),
        re.compile(rf"{step:08d}"),
        re.compile(rf"step[-_]?{step_str}", re.IGNORECASE),
    ]

    matched: list[Path] = []
    for file in sorted(sdir.iterdir()):
        if not file.is_file() or file.suffix.lower() not in (".png", ".jpg", ".jpeg", ".webp"):
            continue
        # Avoid picking up an already generated full grid 'samples.png' if individual samples exist
        if file.name == "samples.png":
            continue
        if any(pat.search(file.stem) for pat in step_patterns):
            matched.append(file)

    if not matched:
        # Fallback: check if 'samples.png' exists directly in step-specific folder or matching name
        direct_grid = sdir / f"samples_step_{step}.png"
        if direct_grid.is_file():
            return [direct_grid]

    return matched


def find_and_compose_step_grid(
    samples_dir: Path | str,
    step: int,
    output_path: Path | str,
) -> Optional[Path]:
    """Locate sample preview images for a step and compose them into output_path."""
    sample_images = find_step_sample_images(samples_dir, step)
    if not sample_images:
        return None

    return stitch_sample_grid(sample_images, output_path)
