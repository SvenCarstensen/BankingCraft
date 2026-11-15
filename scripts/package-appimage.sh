#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
root="$PWD"
version="$(python3 scripts/release-version.py)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
appdir="$work/BankingCraft.AppDir"
mkdir -p "$root/dist" "$appdir/usr/bin"
cp target/release/bankingcraft "$appdir/usr/bin/"
python3 scripts/package-release.py resources --destination "$appdir/usr/share/bankingcraft"

# Released versions and verified digests; update these deliberately when upgrading tooling.
download() {
  curl --fail --location --retry 3 "$1" --output "$work/$2"
  echo "$3  $work/$2" | sha256sum --check --strict
  chmod +x "$work/$2"
}
download https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage linuxdeploy.AppImage c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d
download https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage appimagetool.AppImage ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
download https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64 runtime-x86_64 2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
export APPIMAGE_EXTRACT_AND_RUN=1
"$work/linuxdeploy.AppImage" --appdir "$appdir" \
  --executable "$appdir/usr/bin/bankingcraft" \
  --desktop-file packaging/linux/BankingCraft.desktop \
  --icon-file packaging/linux/bankingcraft.png

# Preserve the distribution's copyright notices for the shared libraries linuxdeploy bundles.
native="$appdir/usr/share/bankingcraft/licenses/linux-system"
mkdir -p "$native"
while IFS= read -r library; do
  system_library="$(ldconfig -p | awk -v name="$(basename "$library")" '$1 == name && !found++ { print $NF }')"
  [ -n "$system_library" ] || continue
  package="$(dpkg-query -S "$system_library" 2>/dev/null | sed -n '1s/: \/.*//p' || true)"
  if [ -z "$package" ]; then
    package="$(dpkg-query -S "$(readlink -f "$system_library")" | sed -n '1s/: \/.*//p')"
  fi
  dpkg-query -W -f='${Package} ${Version} (source: ${source:Package} ${source:Version})\n' "$package" >> "$native/packages.txt"
  copyright="/usr/share/doc/${package%%:*}/copyright"
  test -f "$copyright"
  cp "$copyright" "$native/${package%%:*}-copyright.txt"
done < <(find "$appdir/usr/lib" -type f -name '*.so*')
sort -u -o "$native/packages.txt" "$native/packages.txt"

ARCH=x86_64 VERSION="$version" "$work/appimagetool.AppImage" \
  --runtime-file "$work/runtime-x86_64" "$appdir" \
  "$root/dist/BankingCraft-$version-linux-x86_64.AppImage"
