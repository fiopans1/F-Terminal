#!/usr/bin/env python3
"""Generate F-Terminal icons from a simple vector-style drawing.

This is a development helper. The generated platform assets are committed so
packaging does not depend on Python or Pillow.

Usage:
    python3 scripts/generate-icons.py
"""

from __future__ import annotations

import shutil
import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"
MASTER_SIZE = 1024

BACKGROUND = "#1a1b26"
WINDOW = "#16161e"
BORDER = "#3b4261"
BAR = "#1f2335"
BLUE = "#7aa2f7"
CYAN = "#7dcfff"
RED = "#f7768e"
YELLOW = "#e0af68"
GREEN = "#9ece6a"


def rounded_rectangle(
    draw: ImageDraw.ImageDraw,
    box: tuple[int, int, int, int],
    radius: int,
    fill: str,
    outline: str | None = None,
    width: int = 0,
) -> None:
    draw.rounded_rectangle(box, radius=radius, fill=fill, outline=outline, width=width)


def line_with_caps(
    draw: ImageDraw.ImageDraw,
    start: tuple[int, int],
    end: tuple[int, int],
    fill: str,
    width: int,
) -> None:
    draw.line([start, end], fill=fill, width=width)
    radius = width // 2
    for x, y in (start, end):
        draw.ellipse(
            (x - radius, y - radius, x + radius, y + radius),
            fill=fill,
        )


def build_master() -> Image.Image:
    image = Image.new("RGBA", (MASTER_SIZE, MASTER_SIZE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)

    # Outer rounded square with a subtle border.
    rounded_rectangle(
        draw,
        (48, 48, MASTER_SIZE - 48, MASTER_SIZE - 48),
        radius=210,
        fill=BACKGROUND,
        outline=BORDER,
        width=10,
    )

    # Terminal window.
    rounded_rectangle(
        draw,
        (158, 226, MASTER_SIZE - 158, MASTER_SIZE - 226),
        radius=88,
        fill=WINDOW,
        outline=BORDER,
        width=10,
    )
    draw.rounded_rectangle(
        (158, 226, MASTER_SIZE - 158, 344),
        radius=88,
        fill=BAR,
    )
    draw.rectangle((158, 300, MASTER_SIZE - 158, 344), fill=BAR)
    draw.line((168, 344, MASTER_SIZE - 168, 344), fill=BORDER, width=8)

    # Window controls.
    for index, color in enumerate((RED, YELLOW, GREEN)):
        cx = 250 + index * 70
        cy = 292
        draw.ellipse((cx - 18, cy - 18, cx + 18, cy + 18), fill=color)

    # Prompt: >_
    line_with_caps(draw, (300, 438), (430, 548), BLUE, 66)
    line_with_caps(draw, (430, 548), (300, 658), BLUE, 66)
    line_with_caps(draw, (562, 658), (742, 658), CYAN, 66)

    return image


def save_png(image: Image.Image, path: Path, size: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    resized = image.resize((size, size), Image.Resampling.LANCZOS)
    resized.save(path, format="PNG")


def save_macos(image: Image.Image) -> None:
    icns = ASSETS / "macos" / "AppIcon.icns"
    icns.parent.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="f-terminal-iconset-") as temp:
        iconset = Path(temp) / "AppIcon.iconset"
        iconset.mkdir()

        variants = {
            "icon_16x16.png": 16,
            "icon_16x16@2x.png": 32,
            "icon_32x32.png": 32,
            "icon_32x32@2x.png": 64,
            "icon_128x128.png": 128,
            "icon_128x128@2x.png": 256,
            "icon_256x256.png": 256,
            "icon_256x256@2x.png": 512,
            "icon_512x512.png": 512,
            "icon_512x512@2x.png": 1024,
        }
        for name, size in variants.items():
            save_png(image, iconset / name, size)

        if shutil.which("iconutil"):
            subprocess.run(
                ["iconutil", "-c", "icns", str(iconset), "-o", str(icns)],
                check=True,
            )
        else:
            print("iconutil not found; AppIcon.icns was not regenerated")


def save_windows(image: Image.Image) -> None:
    path = ASSETS / "windows" / "f-terminal.ico"
    path.parent.mkdir(parents=True, exist_ok=True)
    ico_source = image.resize((256, 256), Image.Resampling.LANCZOS)
    ico_source.save(
        path,
        format="ICO",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )


def save_linux(image: Image.Image) -> None:
    for size in (16, 24, 32, 48, 64, 128, 256, 512):
        save_png(
            image,
            ASSETS / "linux" / "hicolor" / f"{size}x{size}" / "apps" / "f-terminal.png",
            size,
        )


def main() -> None:
    image = build_master()
    ASSETS.mkdir(parents=True, exist_ok=True)
    image.save(ASSETS / "icon-1024.png", format="PNG")
    save_png(image, ASSETS / "icon-512.png", 512)
    save_macos(image)
    save_windows(image)
    save_linux(image)
    print(f"icons generated under {ASSETS}")


if __name__ == "__main__":
    main()
