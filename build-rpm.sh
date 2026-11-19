#!/usr/bin/env bash
# Gera o pacote .rpm (Fedora, openSUSE, RHEL...). Não precisa de root.
set -euo pipefail
cd "$(dirname "$0")"

VER=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
echo "=== Zek Music $VER — pacote .rpm ==="

cargo build --release

B=build/rpm
rm -rf "$B"
STAGE="$B/zek-music-$VER"
mkdir -p "$STAGE/packaging/linux" "$B/SOURCES"
cp target/release/zek-music "$STAGE/"
cp LICENSE "$STAGE/"
cp packaging/linux/zek-music.desktop "$STAGE/packaging/linux/"
cp packaging/zek-music.svg packaging/zek-music.png "$STAGE/packaging/"
tar -C "$B" -czf "$B/SOURCES/zek-music-$VER.tar.gz" "zek-music-$VER"

rpmbuild -bb packaging/linux/zek-music.spec \
  --define "_topdir $PWD/$B" \
  --define "_sourcedir $PWD/$B/SOURCES" \
  --define "_rpmdir $PWD/build/dist"

mkdir -p build/dist
RPM=$(find build/dist -name "zek-music-$VER-*.rpm" | head -1)
echo "Pronto: $RPM"
echo "Instalar com:  sudo dnf install \"$RPM\""