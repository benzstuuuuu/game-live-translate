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

# card.png: a whole 1080p card frame for the box picker (#34). Two lines of
# dialogue inside the default box (rows 799 to 979) and a HUD prompt far above.
card = Image.new("RGB", (1920, 1080), (34, 40, 52))
d = ImageDraw.Draw(card)
d.rectangle((0, 0, 1919, 700), fill=(52, 70, 60))
d.text((1700, 60), "Press X to skip", font=font, fill=(200, 200, 200), anchor="mt")
d.rectangle((240, 799, 1679, 978), fill=(18, 20, 26))
for j, text in enumerate(["You sure about this,", "Mara?"]):
    d.text((960, 840 + j * 56), text, font=font, fill=(240, 240, 240), anchor="mt")
card.save(Path(__file__).with_name("card.png"), optimize=True)
