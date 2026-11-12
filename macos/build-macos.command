#!/bin/bash
# Baut Kontor als macOS-App (Kontor.app neben dem Projektordner) und startet sie.
DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP="$(cd "$DIR/.." && pwd)/Kontor.app"
LOG="$(cd "$DIR/.." && pwd)/kontor-build.log"
cd "$DIR" || exit 1
LOCK="${TMPDIR:-/tmp}/kontor-build.lock"
if ! mkdir "$LOCK" 2>/dev/null; then echo "Build läuft bereits."; exit 0; fi
trap 'rmdir "$LOCK"' EXIT
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

{
  echo "== Kontor macOS-Build $(date)"
  sw_vers 2>/dev/null; uname -m
  cargo --version && rustc --version
} 2>&1 | tee "$LOG"

if ! command -v cargo >/dev/null; then
  echo "FEHLER: cargo nicht gefunden (rustup installieren: https://rustup.rs)" | tee -a "$LOG"
  read -r -p "Enter zum Schließen"; exit 1
fi

cargo build --release 2>&1 | tee -a "$LOG"
if [ "${PIPESTATUS[0]}" -ne 0 ]; then
  echo "BUILD FAILED" | tee -a "$LOG"
  read -r -p "Enter zum Schließen"; exit 1
fi

# laufende Instanz beenden, damit die neue Version startet
if pgrep -x Kontor >/dev/null; then
  osascript -e 'tell application id "de.appcreation.kontor" to quit' 2>/dev/null
  for _ in 1 2 3 4 5 6 7 8 9 10; do pgrep -x Kontor >/dev/null || break; sleep 0.5; done
fi
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/kontor "$APP/Contents/MacOS/Kontor"
cp macos/Info.plist "$APP/Contents/Info.plist"

# App-Icon aus dem PNG erzeugen
ICONSET="$(mktemp -d)/Kontor.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s macos/icon-1024.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s*2)) $((s*2)) macos/icon-1024.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Kontor.icns" 2>&1 | tee -a "$LOG"

# Ad-hoc-Signatur (lokal ausreichend, nicht notarisiert)
codesign --force --deep --sign - "$APP" 2>&1 | tee -a "$LOG"

echo "BUILD OK: $APP" | tee -a "$LOG"
open "$APP"
