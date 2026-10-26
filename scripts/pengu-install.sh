#!/bin/bash
# Installs (or restores) the Pengu loader into the League client on macOS.
#   pengu-install.sh            install / refresh
#   pengu-install.sh restore    put the original libEGL.dylib back
# Run again after every League update (the client replaces libEGL.dylib).
set -euo pipefail

PENGU_DIR="${PENGU_DIR:-$HOME/Desktop/game/Pengu}"
PLUGIN_SRC="$(cd "$(dirname "$0")/.." && pwd)/client-plugin"
HOME_DIR=/Users/Shared/nebula   # short on purpose: libEGL has ~56 spare header bytes
LIBS="/Applications/League of Legends.app/Contents/LoL/League of Legends.app/Contents/Frameworks/Chromium Embedded Framework.framework/Libraries"
TARGET="$LIBS/libEGL.dylib"
ORIG="$HOME_DIR/libEGL.dylib.orig"

running="$(ps -ax -o comm= | sed 's#.*/##' | grep -E '^(LeagueClient|LeagueClientUx|RiotClientServices|Riot Client)$' | sort -u | tr '\n' ' ' || true)"
if [ -n "$running" ]; then
  echo "Close League and the Riot Client first (still running: $running)" >&2; exit 1
fi

if [ "${1:-}" = restore ]; then
  cp "$ORIG" "$TARGET" && echo "Original libEGL.dylib restored."; exit 0
fi

mkdir -p "$HOME_DIR/plugins/nebula"
cp "$PENGU_DIR/bin/core.dylib" "$HOME_DIR/core.dylib"
codesign --force --sign - "$HOME_DIR/core.dylib"
PLUGIN_DST="$HOME_DIR/plugins/nebula"
mkdir -p "$PLUGIN_DST"
cp "$PLUGIN_SRC"/*.js "$PLUGIN_DST/"
# shared secret between the plugin and the Nebula daemon; kept across reinstalls
[ -s "$PLUGIN_DST/token.js" ] || printf 'export default "%s";\n' "$(openssl rand -hex 16)" > "$PLUGIN_DST/token.js"
chmod 644 "$PLUGIN_DST"/*.js

if otool -L "$TARGET" | grep -q "$HOME_DIR/core.dylib"; then
  echo "libEGL.dylib already patched; plugins refreshed."; exit 0
fi

cp "$TARGET" "$ORIG"                      # the client's current, unpatched copy
WORK="$(mktemp -d)"
TMP="$WORK/libEGL.dylib"

# Only the arm64 slice gets the new load command: the x86_64 slice has too little
# header padding and would have its first code bytes overwritten. Keep it as shipped.
archs="$(lipo -archs "$ORIG")"
case " $archs " in *" arm64 "*) ;; *) echo "No arm64 slice in libEGL.dylib; this installer targets Apple Silicon." >&2; exit 1;; esac
lipo "$ORIG" -thin arm64 -output "$WORK/arm64"
"$PENGU_DIR/bin/insert_dylib" --all-yes --inplace "$HOME_DIR/core.dylib" "$WORK/arm64"

# The new load command must end before the first section, or it ate real code.
read -r ncmds sizeofcmds < <(otool -h "$WORK/arm64" | tail -1 | awk '{print $6, $7}')
text_off="$(otool -l "$WORK/arm64" | awk '/sectname __text/{f=1} f&&/ offset /{print $2; exit}')"
if [ $((32 + sizeofcmds)) -gt "$text_off" ]; then
  echo "Load commands overlap __text; refusing to patch." >&2; exit 1
fi

if [[ " $archs " == *" x86_64 "* ]]; then
  lipo "$ORIG" -thin x86_64 -output "$WORK/x86_64"
  lipo -create "$WORK/arm64" "$WORK/x86_64" -output "$TMP"
else
  cp "$WORK/arm64" "$TMP"
fi
codesign --force --sign - "$TMP"          # arm64 refuses unsigned code
codesign -v "$TMP"
cp "$TMP" "$TARGET"
echo "Pengu installed. Open League normally."
