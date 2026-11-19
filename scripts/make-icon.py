"""Generate RideShell's icon (PNG + multi-size ICO).

Usage: python scripts/make-icon.py
Writes crates/rideshell-app/assets/icon.png, icon-256.png and crates/rideshell-app/icon.ico.

Design: a dark, slightly glassy rounded tile with a violet -> cyan glow, a bold prompt
chevron, and a gradient cursor bar: "a terminal, but newer".
"""

from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "crates" / "rideshell-app"
S = 1024  # master size; everything is drawn at 4x and downsampled
SS = 4
W = S * SS

VIOLET = (139, 92, 246)
CYAN = (34, 211, 238)
PINK = (236, 72, 153)


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def gradient(size, c1, c2, diagonal=True):
    """Diagonal gradient from top-left c1 to bottom-right c2."""
    w, h = size
    g = Image.new("RGB", (256, 256))
    px = g.load()
    for y in range(256):
        for x in range(256):
            t = ((x + y) / 510) if diagonal else (x / 255)
            px[x, y] = lerp(c1, c2, t)
    return g.resize((w, h), Image.BICUBIC)


def rounded_mask(size, radius):
    m = Image.new("L", size, 0)
    ImageDraw.Draw(m).rounded_rectangle((0, 0, size[0] - 1, size[1] - 1), radius=radius, fill=255)
    return m


def build():
    img = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    margin = int(W * 0.06)
    tile = (W - 2 * margin, W - 2 * margin)
    radius = int(tile[0] * 0.235)
    mask = rounded_mask(tile, radius)

    # Base: deep navy with a soft vertical falloff.
    base = gradient(tile, (24, 22, 48), (8, 10, 22))
    # Glow blobs: violet top-left, cyan bottom-right.
    glow = Image.new("RGB", tile, (0, 0, 0))
    gd = ImageDraw.Draw(glow)
    gd.ellipse((-tile[0] * 0.25, -tile[1] * 0.35, tile[0] * 0.75, tile[1] * 0.55), fill=VIOLET)
    gd.ellipse((tile[0] * 0.35, tile[1] * 0.45, tile[0] * 1.3, tile[1] * 1.3), fill=CYAN)
    glow = glow.filter(ImageFilter.GaussianBlur(tile[0] * 0.16))
    base = Image.blend(base, ImageChops.screen(base, glow), 0.55)

    tile_img = Image.new("RGBA", tile)
    tile_img.paste(base, (0, 0))
    tile_img.putalpha(mask)

    # Gradient rim (1.2% of size), brighter at the top: a glass edge.
    rim_w = max(2, int(tile[0] * 0.012))
    inner = Image.new("L", tile, 0)
    ImageDraw.Draw(inner).rounded_rectangle(
        (rim_w, rim_w, tile[0] - 1 - rim_w, tile[1] - 1 - rim_w), radius=radius - rim_w, fill=255
    )
    rim_mask = ImageChops.subtract(mask, inner)
    rim = gradient(tile, (196, 181, 253), (103, 232, 249))
    rim_rgba = Image.new("RGBA", tile)
    rim_rgba.paste(rim, (0, 0))
    rim_rgba.putalpha(rim_mask.point(lambda v: int(v * 0.85)))
    tile_img = Image.alpha_composite(tile_img, rim_rgba)

    # Top sheen.
    sheen = Image.new("L", tile, 0)
    ImageDraw.Draw(sheen).ellipse((-tile[0] * 0.2, -tile[1] * 0.75, tile[0] * 1.2, tile[1] * 0.42), fill=34)
    sheen = ImageChops.multiply(sheen.filter(ImageFilter.GaussianBlur(tile[0] * 0.03)), mask)
    white = Image.new("RGBA", tile, (255, 255, 255, 0))
    white.putalpha(sheen)
    tile_img = Image.alpha_composite(tile_img, white)

    # Glyphs: chevron and cursor bar, with a soft glow behind them.
    glyph = Image.new("L", tile, 0)
    d = ImageDraw.Draw(glyph)
    tw, th = tile
    stroke = int(tw * 0.105)
    cx, cy = tw * 0.33, th * 0.50
    arm = tw * 0.165
    pts = [(cx - arm * 0.62, cy - arm), (cx + arm * 0.62, cy), (cx - arm * 0.62, cy + arm)]
    d.line(pts, fill=255, width=stroke, joint="curve")
    for p in (pts[0], pts[2]):
        d.ellipse((p[0] - stroke / 2, p[1] - stroke / 2, p[0] + stroke / 2, p[1] + stroke / 2), fill=255)

    bar = Image.new("L", tile, 0)
    bd = ImageDraw.Draw(bar)
    bx0, by0 = tw * 0.52, th * 0.60
    bx1, by1 = tw * 0.76, th * 0.60 + stroke
    bd.rounded_rectangle((bx0, by0, bx1, by1), radius=stroke / 2, fill=255)

    halo = ImageChops.lighter(glyph, bar).filter(ImageFilter.GaussianBlur(tw * 0.035))
    halo_rgba = Image.new("RGBA", tile, (167, 139, 250, 0))
    halo_rgba.putalpha(halo.point(lambda v: int(v * 0.55)))
    tile_img = Image.alpha_composite(tile_img, halo_rgba)

    chevron = Image.new("RGBA", tile, (248, 250, 252, 0))
    chevron.putalpha(glyph)
    tile_img = Image.alpha_composite(tile_img, chevron)

    # The bar's own gradient spans just the bar: vivid at both ends.
    bar_fill = Image.new("RGBA", tile, (0, 0, 0, 0))
    span = (int(bx1 - bx0) + 1, int(by1 - by0) + 1)
    bar_fill.paste(gradient(span, PINK, CYAN, diagonal=False).convert("RGBA"), (int(bx0), int(by0)))
    bar_fill.putalpha(bar)
    tile_img = Image.alpha_composite(tile_img, bar_fill)

    # Drop shadow under the tile.
    shadow = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    sh = Image.new("L", (W, W), 0)
    sh.paste(mask.point(lambda v: int(v * 0.45)), (margin, margin + int(W * 0.012)))
    shadow.putalpha(sh.filter(ImageFilter.GaussianBlur(W * 0.015)))
    img = Image.alpha_composite(img, shadow)
    img.alpha_composite(tile_img, (margin, margin))
    return img.resize((S, S), Image.LANCZOS)


def main():
    master = build()
    assets = OUT / "assets"
    assets.mkdir(parents=True, exist_ok=True)
    master.resize((512, 512), Image.LANCZOS).save(assets / "icon.png")
    master.resize((256, 256), Image.LANCZOS).save(assets / "icon-256.png")
    sizes = [(s, s) for s in (16, 20, 24, 32, 40, 48, 64, 128, 256)]
    master.save(OUT / "icon.ico", sizes=sizes)
    print("wrote", assets / "icon.png", "and", OUT / "icon.ico")


if __name__ == "__main__":
    main()
