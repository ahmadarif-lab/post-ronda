#!/usr/bin/env python3
"""Shrinks the bundled Inter font to what the interface draws.

Keeps Latin (including Indonesian and Western European text), punctuation, currency, arrows,
math and keyboard symbols (⌘ ⌥ ⇧ ↵), check marks and shapes, and the full weight/optical-size
axes. Other scripts still render through egui's built-in fallback fonts.
Reads assets/src/InterVariable-full.ttf, writes assets/InterVariable.ttf. Needs fontTools.
"""
import pathlib
from fontTools import subset

root = pathlib.Path(__file__).resolve().parent.parent
ranges = [
    (0x0020, 0x007E), (0x00A0, 0x00FF), (0x0100, 0x017F), (0x0180, 0x024F),
    (0x02B0, 0x02FF), (0x0300, 0x036F), (0x2000, 0x206F), (0x2070, 0x209F),
    (0x20A0, 0x20CF), (0x2100, 0x214F), (0x2150, 0x218F), (0x2190, 0x21FF),
    (0x2200, 0x22FF), (0x2300, 0x23FF), (0x2420, 0x2426), (0x2500, 0x257F),
    (0x25A0, 0x25FF), (0x2600, 0x26FF), (0x2700, 0x27BF), (0xFB00, 0xFB06),
    (0xFFFD, 0xFFFD),
]
unicodes = [c for lo, hi in ranges for c in range(lo, hi + 1)]
options = subset.Options()
options.layout_features = ["kern", "liga", "calt", "ccmp", "locl", "mark", "mkmk", "tnum", "case", "zero"]
options.glyph_names = False
options.notdef_outline = True
options.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14]
options.drop_tables += ["STAT"]
source = root / "assets" / "src" / "InterVariable-full.ttf"
font = subset.load_font(str(source), options)
subsetter = subset.Subsetter(options)
subsetter.populate(unicodes=unicodes)
subsetter.subset(font)
target = root / "assets" / "InterVariable.ttf"
subset.save_font(font, str(target), options)
print(f"{source.stat().st_size // 1024} KB -> {target.stat().st_size // 1024} KB")
