#!/usr/bin/env python3
"""Generate the comind brand assets (Botanical petal-ring) into assets/.

Emits SVGs (source of truth) for: the mark (light/dark), a self-contained tiled icon,
horizontal wordmark lockups (light/dark), and a social/OG banner. Run scripts/render_pngs.sh
afterwards to rasterize the no-text marks to PNG.

    python3 scripts/gen_brand.py
"""
import math
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "assets")

# ---- palette -------------------------------------------------------------------------------
LIGHT = ["#0D9488", "#16A34A", "#65A30D"]   # deep teal→green→lime — visible on white
DARK = ["#2DD4BF", "#4ADE80", "#A3E635"]    # bright teal→green→lime — pops on dark
INK_LIGHT = "#16202E"
INK_DARK = "#EEF2F7"
TILE = "#0B1120"                             # deep ink ground for the self-contained icon

N = 10           # petals
C = 128          # center (256 viewBox)
R = 104          # ring radius
RX, RY = 10, 21  # petal half-axes
PW = 4           # petal stroke width


def _rgb(h):
    h = h.lstrip("#")
    return [int(h[i:i + 2], 16) for i in (0, 2, 4)]


def _hex(r):
    return "#" + "".join(f"{round(v):02x}" for v in r)


def ramp_at(stops, t):
    if len(stops) == 1:
        return stops[0]
    g = t * (len(stops) - 1)
    i = min(int(g), len(stops) - 2)
    f = g - i
    a, b = _rgb(stops[i]), _rgb(stops[i + 1])
    return _hex([a[k] + (b[k] - a[k]) * f for k in range(3)])


def petals(stops, cx=C, cy=C, r=R, rx=RX, ry=RY, sw=PW):
    out = []
    for i in range(N):
        a = -math.pi / 2 + i * 2 * math.pi / N
        x, y = cx + r * math.cos(a), cy + r * math.sin(a)
        deg = a * 180 / math.pi + 90
        col = ramp_at(stops, i / (N - 1))
        out.append(
            f'<ellipse cx="{x:.1f}" cy="{y:.1f}" rx="{rx}" ry="{ry}" '
            f'transform="rotate({deg:.1f} {x:.1f} {y:.1f})" fill="none" '
            f'stroke="{col}" stroke-width="{sw}"/>'
        )
    return "\n    ".join(out)


def graph(ink, scale=0.46, cx=C, cy=C):
    lines = [(177.23, 111.59, 150.77, 120.41), (181.06, 169.27, 146.94, 142.73),
             (110.06, 143.94, 73.94, 176.06), (118.25, 106.07, 105.75, 77.93)]
    nodes = [(128, 128), (200, 104), (200, 184), (56, 192), (96, 56)]
    inner = "".join(f'<line x1="{a}" y1="{b}" x2="{c}" y2="{d}" stroke="{ink}"/>' for a, b, c, d in lines)
    inner += "".join(f'<circle cx="{x}" cy="{y}" r="24" stroke="{ink}"/>' for x, y in nodes)
    return (f'<g transform="translate({cx} {cy}) scale({scale}) translate(-128 -124)" '
            f'fill="none" stroke-linecap="round" stroke-linejoin="round" stroke-width="17">{inner}</g>')


def mark_svg(stops, ink):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256" '
            f'role="img" aria-label="comind">\n  <title>comind</title>\n'
            f'  <!-- Botanical petal ring; graph mark adapted from Phosphor Icons "graph" (MIT) -->\n'
            f'  <g>\n    {petals(stops)}\n  </g>\n  {graph(ink)}\n</svg>\n')


def tiled_icon_svg():
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256" '
            f'role="img" aria-label="comind">\n  <title>comind</title>\n'
            f'  <rect x="0" y="0" width="256" height="256" rx="58" fill="{TILE}"/>\n'
            f'  <g transform="translate(128 128) scale(0.86) translate(-128 -128)">\n'
            f'    <g>\n    {petals(DARK)}\n    </g>\n    {graph(INK_DARK)}\n  </g>\n</svg>\n')


def wordmark_svg(stops, ink, bg=None):
    # horizontal lockup: mark (scaled to ~120) + "comind" wordmark
    w, h = 560, 180
    bgrect = f'<rect width="{w}" height="{h}" fill="{bg}"/>' if bg else ""
    mark = (f'<g transform="translate(18 30) scale(0.47)">'
            f'<g>{petals(stops)}</g>{graph(ink)}</g>')
    text = (f'<text x="182" y="90" dominant-baseline="central" '
            f'font-family="\'JetBrains Mono\', ui-monospace, SFMono-Regular, Menlo, monospace" '
            f'font-weight="700" font-size="76" letter-spacing="-2" fill="{ink}">comind</text>')
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" '
            f'role="img" aria-label="comind">\n  <title>comind</title>\n  {bgrect}\n  {mark}\n  {text}\n</svg>\n')


TAGLINE = "deterministic, cross-repo code intelligence for agents"


def lockup_svg(stops, ink, bg=None):
    # horizontal lockup with slogan: mark + "comind" over the tagline.
    w, h = 760, 180
    bgrect = f'<rect width="{w}" height="{h}" fill="{bg}"/>' if bg else ""
    mark = (f'<g transform="translate(20 30) scale(0.47)"><g>{petals(stops)}</g>{graph(ink)}</g>')
    word = (f'<text x="186" y="82" font-family="\'JetBrains Mono\', ui-monospace, monospace" '
            f'font-weight="700" font-size="62" letter-spacing="-2" fill="{ink}">comind</text>')
    tag = (f'<text x="188" y="120" font-family="ui-sans-serif, system-ui, sans-serif" '
           f'font-size="21" letter-spacing=".2" fill="#8092a8">{TAGLINE}</text>')
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" '
            f'role="img" aria-label="comind — {TAGLINE}">\n  <title>comind</title>\n  {bgrect}\n'
            f'  {mark}\n  {word}\n  {tag}\n</svg>\n')


def og_svg():
    # 1200x630 social card: mark centered on top, wordmark, then tagline — stacked, no overlap.
    w, h = 1200, 630
    mark = (f'<g transform="translate(440 96) scale(1.25)"><g>{petals(DARK)}</g>{graph(INK_DARK)}</g>')
    word = (f'<text x="600" y="500" text-anchor="middle" '
            f'font-family="\'JetBrains Mono\', ui-monospace, monospace" font-weight="700" '
            f'font-size="92" letter-spacing="-3" fill="{INK_DARK}">comind</text>')
    tag = (f'<text x="600" y="556" text-anchor="middle" '
           f'font-family="ui-sans-serif, system-ui, sans-serif" font-size="28" '
           f'letter-spacing="1" fill="#8aa0b8">{TAGLINE}</text>')
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" '
            f'role="img" aria-label="comind">\n  <title>comind</title>\n'
            f'  <rect width="{w}" height="{h}" fill="{TILE}"/>\n  {mark}\n  {word}\n  {tag}\n</svg>\n')


def write(name, content):
    path = os.path.join(OUT, name)
    with open(path, "w") as f:
        f.write(content)
    print("wrote", os.path.relpath(path, ROOT))


def main():
    os.makedirs(OUT, exist_ok=True)
    write("logo.svg", mark_svg(LIGHT, INK_LIGHT))         # mark, light bg
    write("logo-dark.svg", mark_svg(DARK, INK_DARK))      # mark, dark bg
    write("icon.svg", tiled_icon_svg())                   # self-contained favicon/app icon
    write("wordmark.svg", wordmark_svg(LIGHT, INK_LIGHT))       # mark + comind, light
    write("wordmark-dark.svg", wordmark_svg(DARK, INK_DARK))    # mark + comind, dark
    write("lockup.svg", lockup_svg(LIGHT, INK_LIGHT))          # mark + comind + tagline, light
    write("lockup-dark.svg", lockup_svg(DARK, INK_DARK))       # mark + comind + tagline, dark
    write("og.svg", og_svg())                             # social banner 1200x630


if __name__ == "__main__":
    main()
