#!/usr/bin/env bash
# Halftone UI screenshot matrix (pass 2).
# Requires: http server serving ui/ as root on 127.0.0.1:8791 (http_serve.py).
set -u
CHROME=~/.cache/ms-playwright/chromium-1134/chrome-linux/chrome
BASE="http://127.0.0.1:8791/test/ui_harness.html"
OUT="$(cd "$(dirname "$0")" && pwd)/shots"
mkdir -p "$OUT"

shot(){ # $1 rel-url  $2 outfile  $3 wxh
  local url="$BASE?$1"
  "$CHROME" --headless=new --no-sandbox --disable-gpu --hide-scrollbars \
    --window-size="$3" --virtual-time-budget=12000 \
    --screenshot="$OUT/$2" "$url" 2>/dev/null
  echo "ok $2"
}

# main theme x mode (4)
for th in analogue digital; do for mo in dark light; do
  shot "page=main&nav=left&theme=$th&mode=$mo&view=tracks&track=0" "main_${th}_${mo}.png" 1200,760
done; done

# nav positions (5) — analogue dark
for pos in left right top bottom hidden; do
  shot "page=main&nav=$pos&theme=analogue&mode=dark&view=tracks&track=0" "main_nav_${pos}.png" 1200,760
done

# widget presets x sizes — analogue dark (12)
for p in card strip square lyrics; do for wh in 380x260 900x560 1200x800; do
  w=${wh%x*}; h=${wh#*x}
  shot "page=widget&w=$w&h=$h&preset=$p&theme=analogue&mode=dark" "widget_${p}_${wh}.png" "$w,$h"
done; done

# widget digital light+dark at 600x380
for mo in light dark; do
  shot "page=widget&w=600&h=380&preset=card&theme=digital&mode=$mo" "widget_digital_${mo}_600x380.png" 600,380
done

# widget analogue light 600x380 (parity with pass-1 gallery)
shot "page=widget&w=600&h=380&preset=card&theme=analogue&mode=light" "widget_analogue_light_600x380.png" 600,380

# settings view (generated + searchable)
shot "page=main&nav=left&theme=analogue&mode=dark&view=settings" "settings.png" 1200,760

# track sheet + playlist sub-sheet + cover flows
shot "page=main&nav=left&theme=analogue&mode=dark&view=tracks&track=0&sheet=1" "sheet.png" 1200,760
shot "page=main&nav=left&theme=analogue&mode=dark&view=tracks&track=0&sheet=pl" "sheet_playlist.png" 1200,760
shot "page=main&nav=left&theme=analogue&mode=dark&view=tracks&track=0&ctx=1" "main_cover_ctx.png" 1200,760
shot "page=main&nav=left&theme=analogue&mode=dark&view=tracks&track=0&sheet=cover" "main_cover_sheet.png" 1200,760
shot "page=main&nav=left&theme=analogue&mode=dark&view=tracks&track=0&sheet=search" "main_cover_search.png" 1200,760

# lyrics states: synced (track 0), plain (track 1), none (track 2)
shot "page=main&nav=left&theme=analogue&mode=dark&view=nowplaying&track=0" "lyrics_synced.png" 1200,760
shot "page=main&nav=left&theme=analogue&mode=dark&view=nowplaying&track=1" "lyrics_plain.png" 1200,760
shot "page=main&nav=left&theme=analogue&mode=dark&view=nowplaying&track=2" "lyrics_none.png" 1200,760

echo DONE
