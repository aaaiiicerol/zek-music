#!/usr/bin/env bash
# Gera ZekMusic-x86_64.AppImage
set -euo pipefail
cd "$(dirname "$0")"

cargo build --release

rm -rf AppDir
mkdir -p AppDir/usr/bin
cp target/release/zek-music AppDir/usr/bin/
cp packaging/zek-music.desktop AppDir/
cp packaging/zek-music.svg AppDir/

cat > AppDir/AppRun << 'RUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/zek-music" "$@"
RUN
chmod +x AppDir/AppRun

if [ ! -x ./appimagetool ]; then
  echo "Baixando appimagetool..."
  curl -L -o appimagetool https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
  chmod +x appimagetool
fi

ARCH=x86_64 ./appimagetool --appimage-extract-and-run AppDir ZekMusic-x86_64.AppImage
echo "Pronto: ZekMusic-x86_64.AppImage"
