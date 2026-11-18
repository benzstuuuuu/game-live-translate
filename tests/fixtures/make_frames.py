"""Regenerate tests/fixtures/frames/*.png: hand-written dialogue drawn as a
subtitle box, no game footage. Run: python3 tests/fixtures/make_frames.py"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

FRAMES = [
    ["Mara", "Keep your lantern low."],          # speaker + line
    ["Mara", "Keep your lantern low."],          # stable -> line
    ["Mara"],                                    # name only -> ignored
    ["Mara", "Keep your lantern low."],          # same line -> nothing
    ["Ren", "You sure about this,", "Mara?"],    # two-line dialogue
    ["Ren", "You sure about this,", "Mara?"],    # stable -> line
    [],                                          # box empty
    [],                                          # stable empty -> subtitle-gone
    ["No. But it's the"],                        # typewriter, partial
    ["No. But it's the"],                        # stable -> line
    ["No. But it's the only road left."],        # grows
    ["No. But it's the only road left."],        # stable -> line-updated
    ["No. But it's the only road left"],         # OCR drops the period
    ["No. But it's the only road left"],         # >= 85% similar -> nothing
]

out = Path(__file__).with_name("frames")
out.mkdir(exist_ok=True)
font = ImageFont.truetype("/System/Library/Fonts/Helvetica.ttc", 40)
for i, lines in enumerate(FRAMES, 1):
    img = Image.new("RGB", (1200, 220), (18, 20, 26))
    d = ImageDraw.Draw(img)
    for j, text in enumerate(lines):
        d.text((600, 30 + j * 56), text, font=font, fill=(240, 240, 240), anchor="mt")
    img.save(out / f"{i:02}.png", optimize=True)
