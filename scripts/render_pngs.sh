#!/usr/bin/env bash
# Rasterize the brand SVGs (assets/*.svg) to PNG using a headless Chromium browser, then
# downscale the icon to favicon sizes with ImageMagick. Re-run after scripts/gen_brand.py.
#
#   scripts/render_pngs.sh
#
# Override the browser with $BROWSER if needed (any Chromium: Chrome/Edge/Brave/Chromium).
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=assets/png
mkdir -p "$OUT"

BROWSER="${BROWSER:-}"
if [ -z "$BROWSER" ]; then
  for b in \
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge" \
    "/Applications/Chromium.app/Contents/MacOS/Chromium" \
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser" \
    "$(command -v chromium 2>/dev/null || true)" \
    "$(command -v google-chrome 2>/dev/null || true)"; do
    [ -n "$b" ] && [ -x "$b" ] && BROWSER="$b" && break
  done
fi
[ -z "$BROWSER" ] && { echo "no Chromium browser found; set \$BROWSER"; exit 1; }
echo "browser: $BROWSER"

PROFILE="$(mktemp -d)"
trap 'rm -rf "$PROFILE"' EXIT

# render <svg> <out.png> <css-width> <css-height> <scale>
render() {
  local svg="$1" out="$2" w="$3" h="$4" scale="${5:-2}"
  "$BROWSER" --headless=new --disable-gpu --no-first-run --no-default-browser-check \
    --user-data-dir="$PROFILE" --hide-scrollbars \
    --default-background-color=00000000 --force-device-scale-factor="$scale" \
    --window-size="$w,$h" --screenshot="$PWD/$out" "file://$PWD/$svg" >/dev/null 2>&1
  echo "  $out"
}

echo "rendering marks + icon (2x):"
render assets/icon.svg      "$OUT/icon-512.png"       256 256 2
render assets/logo.svg      "$OUT/logo.png"           256 256 2
render assets/logo-dark.svg "$OUT/logo-dark.png"      256 256 2
echo "rendering lockups (2x) + og (1x):"
render assets/wordmark.svg      "$OUT/wordmark.png"       560 180 2
render assets/wordmark-dark.svg "$OUT/wordmark-dark.png"  560 180 2
render assets/og.svg            "$OUT/og.png"            1200 630 1

echo "downscaling favicon sizes from icon-512:"
for s in 256 128 48 32 16; do
  magick "$OUT/icon-512.png" -resize ${s}x${s} "$OUT/icon-${s}.png"
  echo "  $OUT/icon-${s}.png"
done
echo "done."
