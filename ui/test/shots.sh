#!/usr/bin/env bash
# Halftone UI screenshot matrix (pass 2 — full required set).
# Requires: http server serving THIS worktree's ui/ as root on the port
# below (ui/test/http_serve.py). PORT must point at your own worktree,
# not a shared server: another agent's http_serve on 8791 served the
# ht-ui tree once and every shot was stale. Verify with:
#   curl -s http://127.0.0.1:$PORT/theme.js | grep -c applyAttrs
# (dispatchTheme must show applyAttrs BEFORE readTokens)
set -u
CHROME=~/.cache/ms-playwright/chromium-1134/chrome-linux/chrome
PORT=${PORT:-8811}
BASE="http://127.0.0.1:$PORT/test/ui_harness.html"
OUT="$(cd "$(dirname "$0")" && pwd)/shots"
mkdir -p "$OUT"

shot(){ # $1 rel-url  $2 outfile  $3 wxh
  local url="$BASE?$1"
  "$CHROME" --headless=new --no-sandbox --disable-gpu --hide-scrollbars \
    --window-size="$3" --virtual-time-budget=12000 \
    --screenshot="$OUT/$2" "$url" 2>/dev/null
  echo "ok $2"
}

# main theme x mode (4) at 1280x800 AND 1920x1080
for th in analogue digital; do for mo in dark light; do
  shot "page=main&nav=left&theme=$th&mode=$mo&view=tracks&track=0" "main_${th}_${mo}.png" 1280,800
  shot "page=main&nav=left&theme=$th&mode=$mo&view=tracks&track=0" "main_${th}_${mo}_1920.png" 1920,1080
done; done

# nav 3-state (expanded / collapsed / hidden) — analogue dark
shot "page=main&nav=left&theme=analogue&mode=dark&view=tracks&track=0" "main_nav_expanded.png" 1280,800
shot "page=main&nav=left&navstate=collapsed&theme=analogue&mode=dark&view=tracks&track=0" "main_nav_collapsed.png" 1280,800
shot "page=main&nav=left&navstate=hidden&theme=analogue&mode=dark&view=tracks&track=0" "main_nav_hidden.png" 1280,800

# now-playing (liquid glass + analogue dark)
shot "page=main&nav=left&theme=digital&mode=dark&view=nowplaying&track=0" "main_digital_dark_nowplaying.png" 1280,800
shot "page=main&nav=left&theme=analogue&mode=light&view=nowplaying&track=0" "main_analogue_light_nowplaying.png" 1280,800

# widget card + strip in all 4 combos (600x380 = smallest full-card size)
for p in card strip; do for th in analogue digital; do for mo in dark light; do
  shot "page=widget&w=600&h=380&preset=$p&theme=$th&mode=$mo" "widget_${p}_${th}_${mo}.png" 600,380
done; done; done

# widget size sweep (analogue dark card)
for wh in 380x260 900x560 1200x800; do
  w=${wh%x*}; h=${wh#*x}
  shot "page=widget&w=$w&h=$h&preset=card&theme=analogue&mode=dark" "widget_card_${wh}.png" "$w,$h"
done

# seek visualiser close-ups (LED + gel)
shot "page=main&nav=left&theme=analogue&mode=dark&view=nowplaying&track=0&seekzoom=1" "seek_visualiser_closeup.png" 1280,800
shot "page=main&nav=left&theme=digital&mode=dark&view=nowplaying&track=0&seekzoom=1" "seek_gel_closeup.png" 1280,800

# context menu + sheet in liquid glass and risograph
shot "page=main&nav=left&theme=digital&mode=dark&view=tracks&track=0&ctx=1" "menu_digital_dark.png" 1280,800
shot "page=main&nav=left&theme=analogue&mode=light&view=tracks&track=0&ctx=1" "menu_analogue_light.png" 1280,800
shot "page=main&nav=left&theme=digital&mode=dark&view=tracks&track=0&sheet=1" "sheet_digital_dark.png" 1280,800
shot "page=main&nav=left&theme=analogue&mode=light&view=tracks&track=0&sheet=1" "sheet_analogue_light.png" 1280,800

# lyrics states: synced (track 0), plain (track 1), none (track 2)
shot "page=main&nav=left&theme=analogue&mode=dark&view=nowplaying&track=0" "lyrics_synced.png" 1280,800
shot "page=main&nav=left&theme=analogue&mode=dark&view=nowplaying&track=1" "lyrics_plain.png" 1280,800
shot "page=main&nav=left&theme=analogue&mode=dark&view=nowplaying&track=2" "lyrics_none.png" 1280,800

# settings view
shot "page=main&nav=left&theme=analogue&mode=dark&view=settings" "settings.png" 1280,800

echo DONE
