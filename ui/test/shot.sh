#!/usr/bin/env bash
# Full-page theme gallery screenshots: themes-<theme>-<mode>.png
# chrome --screenshot captures the window (1280x3200 tall); page height is
# auto-clamped by --window-size, so we size the window to the content below.
set -u
CHROME=~/.cache/ms-playwright/chromium-1134/chrome-linux/chrome
GAL=file://$(pwd)/theme_gallery.html
OUT=shots
mkdir -p "$OUT"
for theme in analogue digital; do
  for mode in dark light; do
    "$CHROME" --headless=new --no-sandbox --disable-gpu \
      --hide-scrollbars --force-device-scale-factor=1 \
      --window-size=1280,3200 \
      --screenshot="$PWD/$OUT/themes-$theme-$mode.png" \
      "$GAL?theme=$theme&mode=$mode" 2>/dev/null
    echo "shot: $OUT/themes-$theme-$mode.png"
  done
done
