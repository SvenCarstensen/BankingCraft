#!/bin/bash
# Baut BankingCraft als macOS-App (BankingCraft.app neben dem Projektordner) und startet sie.
DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP="$(cd "$DIR/.." && pwd)/BankingCraft.app"
LOG="$(cd "$DIR/.." && pwd)/bankingcraft-build.log"
cd "$DIR" || exit 1

# Run in the background: minimise this Terminal window right away, close it when done.
WIN_MATCH="build-macos"
term() { osascript -e "tell application \"Terminal\" to $1" >/dev/null 2>&1; }
term "set miniaturized of (every window whose name contains \"$WIN_MATCH\") to true"
build_failed() {
  term "set miniaturized of (every window whose name contains \"$WIN_MATCH\") to false"
  term "activate"
  read -r -p "Enter zum Schließen"
  exit 1
}
LOCK="${TMPDIR:-/tmp}/bankingcraft-build.lock"
if ! mkdir "$LOCK" 2>/dev/null; then echo "Build läuft bereits."; exit 0; fi
trap 'rmdir "$LOCK"' EXIT
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

{
  echo "== BankingCraft macOS-Build $(date)"
  sw_vers 2>/dev/null; uname -m
  cargo --version && rustc --version
} 2>&1 | tee "$LOG"

if ! command -v cargo >/dev/null; then
  echo "FEHLER: cargo nicht gefunden (rustup installieren: https://rustup.rs)" | tee -a "$LOG"
  build_failed
fi

python3 scripts/update-licenses.py --check 2>&1 | tee -a "$LOG"
if [ "${PIPESTATUS[0]}" -ne 0 ]; then
  echo "LICENSE CHECK FAILED" | tee -a "$LOG"
  build_failed
fi

cargo build --release --locked 2>&1 | tee -a "$LOG"
if [ "${PIPESTATUS[0]}" -ne 0 ]; then
  echo "BUILD FAILED" | tee -a "$LOG"
  build_failed
fi

# laufende Instanz beenden, damit die neue Version startet
if pgrep -x BankingCraft >/dev/null; then
  osascript -e 'tell application id "de.appcreation.bankingcraft" to quit' 2>/dev/null
  for _ in 1 2 3 4 5 6 7 8 9 10; do pgrep -x BankingCraft >/dev/null || break; sleep 0.5; done
fi
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/bankingcraft "$APP/Contents/MacOS/BankingCraft"
cp macos/Info.plist "$APP/Contents/Info.plist"
sh scripts/copy-license-bundle.sh "$APP/Contents/Resources" 2>&1 | tee -a "$LOG"
if [ "${PIPESTATUS[0]}" -ne 0 ]; then
  echo "LICENSE COPY FAILED" | tee -a "$LOG"
  build_failed
fi

# App-Icon aus dem PNG erzeugen
ICONSET="$(mktemp -d)/BankingCraft.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s macos/bankingcraft-icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s*2)) $((s*2)) macos/bankingcraft-icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/BankingCraft.icns" 2>&1 | tee -a "$LOG"

# Ad-hoc-Signatur (lokal ausreichend, nicht notarisiert)
codesign --force --deep --sign - "$APP" 2>&1 | tee -a "$LOG"

echo "BUILD OK: $APP" | tee -a "$LOG"
open "$APP"
# close the (finished) Terminal window shortly after this script has exited
( sleep 2; term "close (every window whose name contains \"$WIN_MATCH\")" ) >/dev/null 2>&1 &
disown
