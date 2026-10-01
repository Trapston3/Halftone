#!/bin/sh
# Halftone installer - Linux (x86_64).
#   curl -fsSL https://github.com/Trapston3/Halftone/raw/main/tools/install.sh | sh
#
# Debian/Ubuntu (apt + sudo): installs Halftone_amd64.deb via apt, which pulls
# the webkit2gtk dependencies. Everything else: installs the AppImage to
# ~/.local/bin/halftone (+ menu entry + icon). --appimage forces the AppImage
# path even on apt systems. --uninstall removes Halftone.
#
# Checksums: every release asset has a sibling <asset>.sha256 (sha256sum
# format) uploaded by CI; the download is verified against it.
#
# Downloads try curl first (wget as fallback) with retries and 2s/5s/10s
# backoff between attempts - GitHub's release CDN occasionally answers 504,
# and one attempt is not enough. Candidate URLs per asset, in order:
#   1. the tagged release download URL (when latest.json gave us a version)
#   2. /releases/latest/download/<asset>
#   3. the GitHub API asset browser_download_url
#   4. the GitHub API asset api.github.com url fetched with
#      Accept: application/octet-stream (a different endpoint that skips the
#      CDN redirect which sometimes 504s)
#
# Environment overrides (mainly for testing):
#   HALFTONE_DRYRUN=1     download + verify checksums, but do not install
#   HALFTONE_BASE_URL     fetch latest.json + assets from here instead of
#                         the GitHub release (flat directory; also skips the
#                         API fallback so tests fail deterministically)
#   HALFTONE_PREFIX       install prefix (default ~/.local -> bin/ + share/)
#   HALFTONE_ICON_URL     png used for the menu entry icon
#   HALFTONE_APT_INSTALL  install command for the .deb path
#                         (default: "sudo apt-get install -y")
#   HALFTONE_NO_API=1     never use the GitHub API asset listing
set -eu

REPO="Trapston3/Halftone"
RELEASES_PAGE="https://github.com/$REPO/releases"
BASE_URL="${HALFTONE_BASE_URL:-https://github.com/$REPO/releases/latest/download}"
PREFIX="${HALFTONE_PREFIX:-$HOME/.local}"
SHARE_DIR="${XDG_DATA_HOME:-$PREFIX/share}"
BIN_DIR="$PREFIX/bin"
DESKTOP_DIR="$SHARE_DIR/applications"
ICON_DIR="$SHARE_DIR/icons/hicolor/256x256/apps"
APPIMAGE_DIR="$SHARE_DIR/halftone"
ICON_URL="${HALFTONE_ICON_URL:-https://raw.githubusercontent.com/$REPO/main/src-tauri/icons/256x256.png}"
APT_INSTALL="${HALFTONE_APT_INSTALL:-sudo apt-get install -y}"

BIN="$BIN_DIR/halftone"
DESKTOP_FILE="$DESKTOP_DIR/halftone.desktop"
ICON_FILE="$ICON_DIR/halftone.png"

UA="Halftone-installer"
USE_API=1
[ -n "${HALFTONE_NO_API:-}" ] && USE_API=0
[ -n "${HALFTONE_BASE_URL:-}" ] && USE_API=0
DRYRUN=0
[ "${HALFTONE_DRYRUN:-0}" = "1" ] && DRYRUN=1

LAST_STATUS=""   # last HTTP status code seen during a download ("" if unknown)
LAST_MSG=""      # last download error message
CANDIDATES=""
version=""

usage() {
  cat <<USG
Halftone installer (Linux x86_64)

  usage: install.sh [--appimage] [--uninstall]

    --appimage   force the AppImage install even on apt systems
    --uninstall  remove the binary, menu entry and icon
USG
}

say()  { printf '%s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }

# sleep_backoff N - 2s / 5s / 10s before download attempts 2, 3, 4
sleep_backoff() {
  case "$1" in
    1) sleep 2 ;;
    2) sleep 5 ;;
    *) sleep 10 ;;
  esac
}

# fetch URL OUTFILE - download with retries (github resets idle conns / the
# CDN occasionally 504s). Tries curl, then wget. On failure sets LAST_STATUS
# to the last HTTP status code seen ("" when unknown, e.g. a connect failure)
# and LAST_MSG to the last curl/wget error line.
fetch() {
  LAST_STATUS=""
  if command -v curl >/dev/null 2>&1; then
    code=$(curl $CURL_RETRY_ALL -fsSL -A "$UA" --retry 3 --retry-delay 2 \
             --connect-timeout 15 -o "$2" -w '\n%{http_code}' "$1" 2>"$tmpdir/fetch.err") && return 0
    LAST_STATUS=$(printf '%s' "$code" | tail -n 1)
    case "$LAST_STATUS" in
      [1-9][0-9][0-9]) : ;;
      *) LAST_STATUS="" ;;
    esac
    LAST_MSG=$(tail -n 1 "$tmpdir/fetch.err" 2>/dev/null || true)
    return 1
  fi
  if command -v wget >/dev/null 2>&1; then
    wget -q -U "$UA" --tries=3 --timeout=30 -O "$2" "$1" 2>"$tmpdir/fetch.err" && return 0
    LAST_MSG=$(tail -n 1 "$tmpdir/fetch.err" 2>/dev/null || true)
    return 1
  fi
  warn "neither curl nor wget found - cannot download"
  return 1
}

# sha256_of FILE - lowercase hex digest
sha256_of() {
  sha256sum "$1" | awk '{print tolower($1)}'
}

# sidecar_hash ASSET_URL - digest from the <asset>.sha256 sidecar, or ""
sidecar_hash() {
  curl -fsSL --retry 3 --retry-delay 2 --connect-timeout 15 "$1.sha256" 2>/dev/null |
    awk 'NR==1{print tolower($1)}' || true
}

# verify FILE EXPECTED LABEL - die on mismatch, warn+skip when no checksum
verify() {
  if [ -z "$2" ]; then
    warn "no checksum published for $3 - skipping verification"
    return 0
  fi
  actual=$(sha256_of "$1")
  if [ "$actual" != "$2" ]; then
    die "checksum mismatch for $3
       expected $2
       got      $actual
       (delete the downloaded copy and retry; if this keeps happening,
       report it at $RELEASES_PAGE)"
  fi
  say "  checksum ok: $2"
}

# verify_asset URL FILE NAME - sha256 against the <asset>.sha256 sidecar
# (only meaningful for github.com download URLs)
verify_asset() {
  expected=""
  case "$1" in
    https://github.com/*) expected=$(sidecar_hash "$1") ;;
  esac
  verify "$2" "$expected" "$3"
}

# curl retry-all-errors support (needs curl 7.71+)
CURL_RETRY_ALL=""
if command -v curl >/dev/null 2>&1; then
  v=$(curl --version 2>/dev/null | head -n 1 | awk '{print $2}')
  major=${v%%.*}
  minor=$(printf '%s' "${v#*.}" | cut -d. -f1)
  if [ "${major:-0}" -gt 7 ] || { [ "${major:-0}" -eq 7 ] && [ "${minor:-0}" -ge 71 ]; }; then
    CURL_RETRY_ALL="--retry-all-errors"
  fi
fi

add_candidate() {
  case " $CANDIDATES " in
    *" $1 "*) : ;;
    *) CANDIDATES="$CANDIDATES $1" ;;
  esac
}

ASSET_JSON=""
api_json_fetched=0

# fetch_asset_json - download the GitHub API release listing once
fetch_asset_json() {
  if [ "$api_json_fetched" = 1 ]; then return 0; fi
  if fetch "https://api.github.com/repos/$REPO/releases/latest" "$ASSET_JSON"; then
    api_json_fetched=1
    return 0
  fi
  warn "api listing failed: HTTP status ${LAST_STATUS:-unknown} - using fallback URLs"
  return 1
}

# pick_asset NAME FILE - find the asset named NAME in a GitHub API release
# listing; prints "<browser_download_url> <api.github.com url>"
pick_asset() {
  name=$1
  f=$2
  urls_api=$(sed -n 's/.*"url"[[:space:]]*:[[:space:]]*"\(https:\/\/api\.github\.com\/[^"]*\/releases\/assets\/[0-9][0-9]*\)".*/\1/p' "$f")
  urls_b=$(sed -n 's/.*"browser_download_url"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$f")
  [ -n "$urls_b" ] || return 1
  line=$(printf '%s\n' "$urls_b" | grep -n "/$name\$" | head -n1 | cut -d: -f1)
  [ -n "$line" ] || return 1
  browser=$(printf '%s\n' "$urls_b" | sed -n "${line}p")
  # assets are listed in the same order in both field lists
  api=$(printf '%s\n' "$urls_api" | sed -n "${line}p")
  [ -n "$api" ] || return 1
  printf '%s %s\n' "$browser" "$api"
}

# collect_candidates NAME - set CANDIDATES (see the header for the order).
# With HALFTONE_BASE_URL (or HALFTONE_NO_API=1) the API listing is skipped so
# test overrides fail deterministically.
collect_candidates() {
  name=$1
  CANDIDATES=""
  if [ -n "$version" ] && [ -z "${HALFTONE_BASE_URL:-}" ]; then
    add_candidate "https://github.com/$REPO/releases/download/v$version/$name"
  fi
  add_candidate "$BASE_URL/$name"
  if [ "$USE_API" = 1 ] && fetch_asset_json; then
    pair=$(pick_asset "$name" "$ASSET_JSON" || true)
    if [ -n "$pair" ]; then
      # intentional word split: pair is "<browser_url> <api_url>"
      # shellcheck disable=SC2086
      set -- $pair
      add_candidate "$1"
      add_candidate "$2"
    fi
  fi
}

# download_asset NAME - download a release asset to $tmpdir/NAME using the
# candidate URL list, 4 attempts per URL with 2s/5s/10s backoff, size and
# sha256 verification. Returns 0 on success; on total failure sets
# LAST_STATUS / LAST_MSG and returns 1.
download_asset() {
  name=$1
  out="$tmpdir/$name"
  collect_candidates "$name"
  LAST_STATUS=""
  LAST_MSG="no candidate URLs"
  for c in $CANDIDATES; do
    attempt=1
    while [ "$attempt" -le 4 ]; do
      say "  downloading (attempt $attempt/4): $c"
      rm -f "$out"
      if fetch "$c" "$out"; then
        size=$(wc -c < "$out" | tr -d ' ')
        if [ "$size" -gt 1048576 ]; then
          verify_asset "$c" "$out" "$name"
          return 0
        fi
        LAST_MSG="downloaded file too small ($size bytes)"
        LAST_STATUS=""
      fi
      case "$LAST_STATUS" in
        404) break ;;   # deterministic: try the next candidate URL
      esac
      if [ "$attempt" -lt 4 ]; then sleep_backoff "$attempt"; fi
      attempt=$((attempt + 1))
    done
  done
  return 1
}

# die_download_failed - final, clear error after every attempt failed
die_download_failed() {
  if [ "$LAST_STATUS" = "404" ]; then
    die "the latest Halftone release has no Linux builds yet.
       Linux installers ship with v0.2.0 - meanwhile everything available is at:
         $RELEASES_PAGE"
  fi
  die "download failed (last HTTP status: ${LAST_STATUS:-unknown}).
     GitHub may be having issues - retry in a minute.
     Manual install: download Halftone_amd64.AppImage (or Halftone_amd64.deb)
     from $RELEASES_PAGE and install it by hand (verify against the
     <asset>.sha256 sidecar)."
}

have_fuse2() {
  if command -v ldconfig >/dev/null 2>&1; then
    ldconfig -p 2>/dev/null | grep -q 'libfuse\.so\.2'
  else
    [ -e /usr/lib/x86_64-linux-gnu/libfuse.so.2 ] ||
      [ -e /lib/x86_64-linux-gnu/libfuse.so.2 ]
  fi
}

do_uninstall() {
  removed=0
  for f in "$BIN" "$DESKTOP_FILE" "$ICON_FILE" "$APPIMAGE_DIR/Halftone_amd64.AppImage"; do
    if [ -e "$f" ]; then
      rm -f "$f"
      say "  removed $f"
      removed=1
    fi
  done
  rmdir "$APPIMAGE_DIR" 2>/dev/null || true
  if [ "$removed" = 1 ]; then
    say ""
    say "  Halftone uninstalled."
  else
    say "  nothing to uninstall"
  fi
}

# remove_appimage_install - clean up what install_appimage created, so a
# later .deb install doesn't leave two copies of Halftone on the system
remove_appimage_install() {
  for f in "$BIN" "$DESKTOP_FILE" "$ICON_FILE"; do
    [ -e "$f" ] && rm -f "$f"
  done
  rm -rf "$APPIMAGE_DIR"
}

# dryrun_stop - the download+verify pipeline ran; stop before mutating anything
dryrun_stop() {
  say ""
  say "  dry run OK: download + checksum verified - nothing was installed"
  exit 0
}

install_deb() {
  if [ "$DRYRUN" = 1 ]; then dryrun_stop; fi
  deb="$tmpdir/$DEB_NAME"   # downloaded + verified by download_asset
  remove_appimage_install
  say "  installing via apt (pulls the webkit2gtk dependencies) ..."
  # $APT_INSTALL is intentionally word-split (defaults to several words)
  # shellcheck disable=SC2086
  $APT_INSTALL "$deb" </dev/null
  say ""
  say "  installed. launch 'Halftone' from your app menu, or 'halftone' in a terminal."
  say "  (the menu entry and dependencies are managed by the package)"
}

install_appimage() {
  if [ "$DRYRUN" = 1 ]; then dryrun_stop; fi
  mkdir -p "$BIN_DIR" "$APPIMAGE_DIR"
  rm -rf "$APPIMAGE_DIR"
  mkdir -p "$APPIMAGE_DIR"

  if have_fuse2; then
    # bin/halftone IS the AppImage - it runs normally via FUSE
    rm -f "$BIN"
    install -m 0755 "$tmpdir/$APPIMAGE_NAME" "$BIN"
  else
    # no libfuse2: keep the AppImage out of bin and wrap it so it still runs
    say "  libfuse2 not found - installing a launcher that needs no FUSE"
    install -m 0755 "$tmpdir/$APPIMAGE_NAME" "$APPIMAGE_DIR/$APPIMAGE_NAME"
    rm -f "$BIN"
    {
      printf '#!/bin/sh\n'
      printf '# Halftone launcher (AppImage without FUSE)\n'
      printf 'exec "%s" --appimage-extract-and-run "$@"\n' "$APPIMAGE_DIR/$APPIMAGE_NAME"
    } >"$BIN"
    chmod 0755 "$BIN"
  fi

  mkdir -p "$DESKTOP_DIR" "$ICON_DIR"
  {
    printf '[Desktop Entry]\n'
    printf 'Type=Application\n'
    printf 'Name=Halftone\n'
    printf 'Comment=Dithered local music player\n'
    printf 'Exec=%s\n' "$BIN"
    printf 'Icon=%s\n' "$ICON_FILE"
    printf 'Terminal=false\n'
    printf 'Categories=Audio;AudioVideo;Music;\n'
    printf 'StartupWMClass=com.trapston3.halftone\n'
  } >"$DESKTOP_FILE"
  if fetch "$ICON_URL" "$tmpdir/icon.png"; then
    install -m 0644 "$tmpdir/icon.png" "$ICON_FILE"
  else
    warn "could not download the app icon (non-fatal)"
  fi

  case ":$PATH:" in
    *":$BIN_DIR:"*) : ;;
    *) warn "$BIN_DIR is not on your PATH - add it to ~/.bashrc or ~/.profile:
       export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
  esac
  say ""
  say "  installed. launch: 'halftone' in a terminal, or the Halftone menu entry"
}

main() {
  FORCE_APPIMAGE=0
  ACTION="install"
  for arg in ${1+"$@"}; do
    case "$arg" in
      --appimage) FORCE_APPIMAGE=1 ;;
      --uninstall) ACTION="uninstall" ;;
      -h | --help) usage; exit 0 ;;
      *) die "unknown option: $arg (try --help)" ;;
    esac
  done

  say ""
  say "  HALFTONE - dithered local music player"
  say "  --------------------------------------"

  if [ "$ACTION" = "uninstall" ]; then
    do_uninstall
    return 0
  fi

  case "$(uname -m)" in
    x86_64 | amd64) : ;;
    *)
      die "unsupported architecture: $(uname -m)
       Halftone ships x86_64 Linux builds only. See $RELEASES_PAGE"
      ;;
  esac

  # --- latest.json (best effort: version pins the tagged asset URLs) ---------
  meta_fetched=0
  attempt=1
  while [ "$attempt" -le 4 ]; do
    say "  checking latest.json ... (attempt $attempt/4)"
    if fetch "$BASE_URL/latest.json" "$tmpdir/latest.json"; then
      meta_fetched=1
      break
    fi
    case "$LAST_STATUS" in
      404) break ;;   # deterministic: older release without latest.json
    esac
    if [ "$attempt" -lt 4 ]; then sleep_backoff "$attempt"; fi
    attempt=$((attempt + 1))
  done
  if [ "$meta_fetched" = 1 ]; then
    version=$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$tmpdir/latest.json" | head -n1)
  elif [ "$LAST_STATUS" = "404" ]; then
    say "  no latest.json (older release) - falling back to asset listing"
  else
    say "  couldn't fetch latest.json: ${LAST_MSG:-HTTP status ${LAST_STATUS:-unknown}} - continuing without checksum"
  fi

  ASSET_JSON="$tmpdir/api.json"
  APPIMAGE_NAME="Halftone_amd64.AppImage"
  DEB_NAME="Halftone_amd64.deb"

  if [ "$FORCE_APPIMAGE" = 1 ]; then
    download_asset "$APPIMAGE_NAME" || die_download_failed
    install_appimage
  elif command -v apt-get >/dev/null 2>&1 && command -v sudo >/dev/null 2>&1; then
    if download_asset "$DEB_NAME"; then
      install_deb
    elif [ "$LAST_STATUS" = "404" ]; then
      warn "this release has no .deb - using the AppImage instead"
      download_asset "$APPIMAGE_NAME" || die_download_failed
      install_appimage
    else
      die_download_failed
    fi
  else
    download_asset "$APPIMAGE_NAME" || die_download_failed
    install_appimage
  fi
}

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT
main "$@"
