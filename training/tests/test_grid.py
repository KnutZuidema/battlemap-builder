"""Tests for checkpoint sample grid stitcher."""

from pathlib import Path
from PIL import Image
import pytest

from dungeonforge_training.grid import (
    find_and_compose_step_grid,
    find_step_sample_images,
    stitch_sample_grid,
)


@pytest.fixture
def sample_images(tmp_path: Path):
    imgs = []
    for idx in range(4):
        p = tmp_path / f"test_job_prompt_{idx}_step_250.png"
        im = Image.new("RGB", (64, 64), color=(idx * 40, idx * 50, 100))
        im.save(p)
        imgs.append(p)
    return imgs


def test_stitch_sample_grid_four_images(sample_images, tmp_path: Path):
    output_path = tmp_path / "samples.png"
    result = stitch_sample_grid(sample_images, output_path)

    assert result.is_file()
    with Image.open(result) as grid:
        # 4 images (64x64 each) in 2x2 grid should be 128x128
        assert grid.size == (128, 128)
        assert grid.format == "PNG"


def test_stitch_sample_grid_single_image(tmp_path: Path):
    p = tmp_path / "single.png"
    im = Image.new("RGB", (64, 64), color=(255, 0, 0))
    im.save(p)

    out = tmp_path / "single_out.png"
    stitch_sample_grid([p], out)
    assert out.is_file()
    with Image.open(out) as img:
        assert img.size == (64, 64)


def test_find_and_compose_step_grid(sample_images, tmp_path: Path):
    # sample_images are in tmp_path with names like test_job_prompt_0_step_250.png
    out = tmp_path / "composed_samples.png"
    result = find_and_compose_step_grid(tmp_path, step=250, output_path=out)

    assert result is not None
    assert result.is_file()
    with Image.open(result) as img:
        assert img.size == (128, 128)


def test_find_and_compose_step_grid_not_found(tmp_path: Path):
    out = tmp_path / "not_found.png"
    result = find_and_compose_step_grid(tmp_path, step=999, output_path=out)
    assert result is None
