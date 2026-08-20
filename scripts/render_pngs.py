#!/usr/bin/env python3
"""Rasterize the comind brand assets to PNG — pure Python, no browser.

Draws the petal ring + graph mark directly with Pillow from the same geometry as
scripts/gen_brand.py (imported), supersampled for clean antialiasing. Wordmark/lockup/OG use the
installed JetBrains Mono font when found. Outputs to assets/png/.

    pip install pillow      # once (Pillow is the only dependency)
    python3 scripts/render_pngs.py
"""
import math
import os

from PIL import Image, ImageDraw, ImageFont

import gen_brand as gb

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "assets", "png")
SS = 4  # supersample factor

# ---- fonts (best-effort; SVG stays the canonical text asset) --------------------------------
FONT_DIRS = [os.path.expanduser("~/Library/Fonts"), "/Library/Fonts", "/System/Library/Fonts",
             "/System/Library/Fonts/Supplemental"]


def find_font(names):
    for d in FONT_DIRS:
        for n in names:
            p = os.path.join(d, n)
            if os.path.exists(p):
                return p
    return None


MONO = find_font(["JetBrainsMonoNLNerdFont-Bold.ttf", "JetBrainsMono-Bold.ttf", "Menlo.ttc",
                  "SFNSMono.ttf", "Andale Mono.ttf"])
SANS = find_font(["Helvetica.ttc", "Arial.ttf", "SFNS.ttf", "Verdana.ttf"])


def rgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))


# ---- drawing (256 user-space → pixels) ------------------------------------------------------
def draw_mark(px, stops, ink, inner=1.0):
    """Return an RGBA image (px×px) of the petal ring + graph, scaled by `inner` about center."""
    S = px * SS
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    k = S / 256.0

    def m(x, y):  # 256-space → pixel, applying inner scale about center 128
        return ((128 + inner * (x - 128)) * k, (128 + inner * (y - 128)) * k)

    # petals — a rotated stroked ellipse per node position
    for i in range(gb.N):
        a = -math.pi / 2 + i * 2 * math.pi / gb.N
        cx, cy = 128 + gb.R * math.cos(a), 128 + gb.R * math.sin(a)
        deg = a * 180 / math.pi + 90
        col = rgb(gb.ramp_at(stops, i / (gb.N - 1)))
        rx, ry = gb.RX * inner * k, gb.RY * inner * k
        sw = max(1, int(round(gb.PW * inner * k)))
        pad = int(sw + 4)
        w, h = int(rx * 2 + pad * 2), int(ry * 2 + pad * 2)
        layer = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        ld = ImageDraw.Draw(layer)
        ld.ellipse([pad, pad, pad + rx * 2, pad + ry * 2], outline=col + (255,), width=sw)
        layer = layer.rotate(-deg, resample=Image.BICUBIC, expand=True)
        px_, py_ = m(cx, cy)
        img.alpha_composite(layer, (int(px_ - layer.width / 2), int(py_ - layer.height / 2)))

    # graph — map graph-space → 256-space via its transform, then to pixels
    gs = gb.GRAPH_SCALE
    ink_c = rgb(ink) + (255,)
    sw = max(1, int(round(gb.GRAPH_SW * gs * inner * k)))

    def gm(gx, gy):  # graph space → pixel
        x = 128 + gs * (gx - 128)
        y = 128 + gs * (gy - 124)
        return m(x, y)

    for (x1, y1, x2, y2) in gb.LINES:
        p1, p2 = gm(x1, y1), gm(x2, y2)
        d.line([p1, p2], fill=ink_c, width=sw)
        for p in (p1, p2):  # round caps
            r = sw / 2
            d.ellipse([p[0] - r, p[1] - r, p[0] + r, p[1] + r], fill=ink_c)
    for (nx, ny) in gb.NODES:
        c = gm(nx, ny)
        r = 24 * gs * inner * k
        d.ellipse([c[0] - r, c[1] - r, c[0] + r, c[1] + r], outline=ink_c, width=sw)

    return img.resize((px, px), Image.LANCZOS)


def draw_icon(px):
    """Self-contained tiled icon: dark rounded tile + bright petals + white graph."""
    S = px * SS
    tile = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(tile)
    r = gb.TILE_RADIUS * S / 256.0
    d.rounded_rectangle([0, 0, S - 1, S - 1], radius=r, fill=rgb(gb.TILE) + (255,))
    tile = tile.resize((px, px), Image.LANCZOS)
    mark = draw_mark(px, gb.DARK, gb.INK_DARK, inner=gb.TILE_INNER)
    tile.alpha_composite(mark)
    return tile


def font(path, size):
    if path is None:
        return ImageFont.load_default()
    try:
        return ImageFont.truetype(path, size)
    except Exception:
        return ImageFont.load_default()


def draw_wordmark(stops, ink, tagline=False):
    """Horizontal lockup: mark + 'comind' (+ optional tagline). Transparent background."""
    h = 180 * SS
    mark_px = 120 * SS
    img = Image.new("RGBA", (int(760 * SS), h), (0, 0, 0, 0))
    m = draw_mark(mark_px, stops, ink)
    img.alpha_composite(m, (int(14 * SS), int(30 * SS)))
    d = ImageDraw.Draw(img)
    d.text((int(186 * SS), int(52 * SS)), "comind", font=font(MONO, int(62 * SS)), fill=rgb(ink) + (255,))
    if tagline:
        d.text((int(190 * SS), int(120 * SS)), gb.TAGLINE, font=font(SANS, int(21 * SS)), fill=rgb("#8092a8") + (255,))
    # trim to content width
    bbox = img.getbbox()
    img = img.crop((0, 0, bbox[2] + int(20 * SS), h))
    return img.resize((img.width // SS, img.height // SS), Image.LANCZOS)


def draw_og():
    W, H = 1200 * SS, 630 * SS
    img = Image.new("RGBA", (W, H), rgb(gb.TILE) + (255,))
    mark = draw_mark(int(320 * SS), gb.DARK, gb.INK_DARK)
    img.alpha_composite(mark, ((W - mark.width) // 2, int(90 * SS)))
    d = ImageDraw.Draw(img)
    fw = font(MONO, int(92 * SS))
    tw = d.textlength("comind", font=fw)
    d.text(((W - tw) / 2, int(430 * SS)), "comind", font=fw, fill=rgb(gb.INK_DARK) + (255,))
    ft = font(SANS, int(28 * SS))
    tl = d.textlength(gb.TAGLINE, font=ft)
    d.text(((W - tl) / 2, int(548 * SS)), gb.TAGLINE, font=ft, fill=rgb("#8aa0b8") + (255,))
    return img.resize((1200, 630), Image.LANCZOS)


def save(img, name):
    p = os.path.join(OUT, name)
    img.save(p)
    print("  ", os.path.relpath(p, ROOT))


def main():
    os.makedirs(OUT, exist_ok=True)
    print("marks + icon:")
    save(draw_mark(512, gb.LIGHT, gb.INK_LIGHT), "logo.png")
    save(draw_mark(512, gb.DARK, gb.INK_DARK), "logo-dark.png")
    for s in (512, 256, 128, 64, 48, 32, 16):
        save(draw_icon(s), f"icon-{s}.png")
    print(f"lockups (font: {os.path.basename(MONO) if MONO else 'default'}):")
    save(draw_wordmark(gb.LIGHT, gb.INK_LIGHT), "wordmark.png")
    save(draw_wordmark(gb.DARK, gb.INK_DARK), "wordmark-dark.png")
    save(draw_wordmark(gb.LIGHT, gb.INK_LIGHT, tagline=True), "lockup.png")
    save(draw_wordmark(gb.DARK, gb.INK_DARK, tagline=True), "lockup-dark.png")
    print("social:")
    save(draw_og(), "og.png")
    print("done.")


if __name__ == "__main__":
    main()
