#!/usr/bin/env python3
"""Regenerates the bundled document faces.

One subset per face is produced from one upstream file and written twice:

  crates/opendoc-layout/fonts/*.ttf   the bytes `opendoc-layout` embeds and
                                      measures with (ttf-parser)
  apps/desktop/src/fonts/*.woff2      the bytes the browser renders with

Both encodings carry the *same* `hmtx` advances because both come from the
same subset object in this process. That is what makes Rust's pagination and
Chrome's rendering agree; see docs/adr/0014.

Upstream is Liberation (SIL OFL 1.1, Reserved Font Name "Liberation"). A
subset is a modified version, so the OFL forbids keeping the reserved name —
the family is renamed to "OpenDoc Sans"/"OpenDoc Mono" here.

Requires fonttools (with brotli). Run from anywhere:
    python3 crates/opendoc-layout/fonts/generate.py [--source DIR]
"""

import argparse
import pathlib
import sys

from fontTools import subset
from fontTools.ttLib import TTFont

# Basic Latin, Latin-1, Latin Extended-A, the modifier letters real text uses,
# General Punctuation (quotes, dashes, the space family), currency, a handful
# of symbols word processors emit, and the two f-ligature code points so that
# text containing them precomposed still measures.
UNICODES = (
    "U+0020-007E,U+00A0-00FF,U+0100-017F,U+02C6-02DC,U+2000-206F,"
    "U+20A0-20BF,U+2122,U+2190-2193,U+2202,U+2212,U+2260,U+2264,U+2265,"
    "U+25A0,U+25CF,U+2610,U+2611,U+FB01-FB02"
)

FACES = [
    ("LiberationSans-Regular.ttf", "opendoc-sans-regular", "OpenDoc Sans", "Regular"),
    ("LiberationSans-Bold.ttf", "opendoc-sans-bold", "OpenDoc Sans", "Bold"),
    ("LiberationSans-Italic.ttf", "opendoc-sans-italic", "OpenDoc Sans", "Italic"),
    ("LiberationSans-BoldItalic.ttf", "opendoc-sans-bolditalic", "OpenDoc Sans", "Bold Italic"),
    ("LiberationMono-Regular.ttf", "opendoc-mono-regular", "OpenDoc Mono", "Regular"),
]

# name IDs that carry the family/subfamily/full/PostScript identity
NAME_FAMILY, NAME_SUBFAMILY, NAME_UNIQUE, NAME_FULL = 1, 2, 3, 4
NAME_PS = 6
NAME_TYPO_FAMILY, NAME_TYPO_SUBFAMILY = 16, 17


def rename(font: TTFont, family: str, style: str) -> None:
    full = f"{family} {style}"
    postscript = full.replace(" ", "")
    table = font["name"]
    for record in list(table.names):
        if record.nameID in (NAME_TYPO_FAMILY, NAME_TYPO_SUBFAMILY):
            table.removeNames(nameID=record.nameID)
    for name_id, value in (
        (NAME_FAMILY, family),
        (NAME_SUBFAMILY, style),
        (NAME_UNIQUE, f"{full}; subset of Liberation"),
        (NAME_FULL, full),
        (NAME_PS, postscript),
    ):
        table.setName(value, name_id, 3, 1, 0x409)
        table.setName(value, name_id, 1, 0, 0)


def main() -> int:
    here = pathlib.Path(__file__).resolve().parent
    repo = here.parents[2]
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--source",
        default="/usr/share/fonts/truetype/liberation2",
        help="directory holding the upstream Liberation 2 TTFs",
    )
    args = parser.parse_args()
    source = pathlib.Path(args.source)
    woff_dir = repo / "apps" / "desktop" / "src" / "fonts"
    woff_dir.mkdir(parents=True, exist_ok=True)

    options = subset.Options()
    options.layout_features = []
    options.hinting = False
    options.name_IDs = ["*"]
    options.name_legacy = True
    options.notdef_outline = True
    options.recalc_bounds = True
    options.drop_tables += ["FFTM"]

    for upstream, stem, family, style in FACES:
        path = source / upstream
        if not path.exists():
            print(f"missing upstream face {path}", file=sys.stderr)
            return 1
        font = subset.load_font(str(path), options)
        subsetter = subset.Subsetter(options=options)
        subsetter.populate(unicodes=subset.parse_unicodes(UNICODES))
        subsetter.subset(font)
        rename(font, family, style)
        # TrueType first: the bytes Rust embeds.
        font.flavor = None
        font.save(str(here / f"{stem}.ttf"))
        # Then the same object as WOFF2, for the browser.
        font.flavor = "woff2"
        font.save(str(woff_dir / f"{stem}.woff2"))
        font.close()
        print(f"{stem}: ttf {(here / f'{stem}.ttf').stat().st_size} B, "
              f"woff2 {(woff_dir / f'{stem}.woff2').stat().st_size} B")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
