"""Makes the bundled Barlow fonts use tabular figures by default.

Slint has no `font-feature-settings`, so live numbers (times, BPM, pitch,
countdowns) would be set with proportional digits and twitch as they change
("1" is much narrower than "0"). The fonts carry tabular figures under the
OpenType feature `tnum` (zero.tf … nine.tf, all one width); this script points
the character map of 0–9 at them, so every Text in the app gets equal-width
digits. Only the cmap changes; Barlow has no Reserved Font Name (OFL 1.1).

Run with fontTools (pip install fonttools) on the original fonts:
    python tabular_figures.py BarlowSemiCondensed-*.ttf
It is idempotent: digits already mapped to .tf glyphs stay as they are.
"""

import sys

from fontTools.ttLib import TTFont

DIGITS = "zero one two three four five six seven eight nine".split()


def freeze(path: str) -> None:
    font = TTFont(path)
    glyphs = set(font.getGlyphOrder())
    for table in font["cmap"].tables:
        if not table.isUnicode():
            continue
        for i, name in enumerate(DIGITS):
            cp = ord("0") + i
            if table.cmap.get(cp) == name and f"{name}.tf" in glyphs:
                table.cmap[cp] = f"{name}.tf"
    font.save(path)


if __name__ == "__main__":
    for p in sys.argv[1:]:
        freeze(p)
