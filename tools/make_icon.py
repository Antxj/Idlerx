"""Draws the Idlerx icon: four windows in a grid (the app in one picture).

Generates, from the repository root:
  rust/assets/icon.ico      - executable icon (16..256 px)
  rust/assets/icon_64.rgba  - raw 64x64 RGBA for the window icon
  docs/logo.png             - 256 px for the README

Requires Pillow: py -m pip install pillow
"""

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
SIZE = 1024  # drawn large, then reduced (smooth edges)

BG = (24, 24, 27, 255)
TEAL = (20, 184, 166)
TEAL_LIGHT = (94, 234, 212)


def draw() -> Image.Image:
    img = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((32, 32, SIZE - 32, SIZE - 32), radius=220, fill=BG)

    pad, gap = 200, 56
    cell = (SIZE - 2 * pad - gap) // 2
    for i in range(4):
        r, c = divmod(i, 2)
        x0 = pad + c * (cell + gap)
        y0 = pad + r * (cell + gap)
        color = TEAL_LIGHT if i == 0 else TEAL
        alpha = 255 if i in (0, 3) else 170
        d.rounded_rectangle(
            (x0, y0, x0 + cell, y0 + cell), radius=64, fill=color + (alpha,)
        )
        # window title strip
        d.rounded_rectangle(
            (x0 + 36, y0 + 36, x0 + cell - 36, y0 + 76),
            radius=20,
            fill=(255, 255, 255, 90),
        )
    return img


def main() -> None:
    big = draw()
    assets = ROOT / "rust" / "assets"
    assets.mkdir(parents=True, exist_ok=True)
    sizes = [16, 24, 32, 48, 64, 128, 256]
    big.resize((256, 256), Image.LANCZOS).save(
        assets / "icon.ico", sizes=[(s, s) for s in sizes]
    )
    small = big.resize((64, 64), Image.LANCZOS)
    (assets / "icon_64.rgba").write_bytes(small.tobytes())
    (ROOT / "docs").mkdir(exist_ok=True)
    big.resize((256, 256), Image.LANCZOS).save(ROOT / "docs" / "logo.png")


if __name__ == "__main__":
    main()
