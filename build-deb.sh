#!/usr/bin/env bash
# Gera o pacote .deb (Debian, Ubuntu, Mint, Pop!_OS...) sem precisar de root
# nem do dpkg-deb: monta o pacote na mão com ar + tar.
set -euo pipefail
cd "$(dirname "$0")"

VER=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
echo "=== Zek Music $VER — pacote .deb ==="

cargo build --release

R=build/deb
rm -rf "$R"
mkdir -p "$R/DEBIAN" "$R/usr/bin" "$R/usr/share/applications" \
         "$R/usr/share/icons/hicolor/scalable/apps" "$R/usr/share/icons/hicolor/512x512/apps"

install -Dm0755 target/release/zek-music "$R/usr/bin/zek-music"
install -Dm0644 packaging/linux/zek-music.desktop "$R/usr/share/applications/zek-music.desktop"
install -Dm0644 packaging/zek-music.svg "$R/usr/share/icons/hicolor/scalable/apps/zek-music.svg"
install -Dm0644 packaging/zek-music.png "$R/usr/share/icons/hicolor/512x512/apps/zek-music.png"

TAM=$(du -sk "$R/usr" | cut -f1)
cat > "$R/DEBIAN/control" <<EOF
Package: zek-music
Version: $VER
Architecture: amd64
Maintainer: aaaiiicerol <270046641+aaaiiicerol@users.noreply.github.com>
Installed-Size: $TAM
Depends: libc6, libasound2, libdbus-1-3, libxkbcommon0, libwayland-client0, libgl1
Section: sound
Priority: optional
Homepage: https://github.com/aaaiiicerol/zek-music
Description: player de músicas leve onde cada pasta vira uma playlist
 O Zek Music é um player de músicas offline feito em Rust. Você aponta para
 uma pasta e cada subpasta com músicas vira uma playlist automaticamente.
 Ele acompanha as mudanças nas pastas enquanto você joga.
EOF

# registra o "Abrir com o Zek Music" na instalação
cat > "$R/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
update-desktop-database /usr/share/applications 2>/dev/null || true
update-mime-database /usr/share/mime 2>/dev/null || true
xdg-mime default zek-music.desktop audio/mpeg audio/flac audio/ogg audio/wav audio/mp4 2>/dev/null || true
exit 0
EOF
chmod 0755 "$R/DEBIAN/postinst"

# as peças do .deb ficam numa pasta à parte, para não sujarem a de saída
P=build/pecas-deb
rm -rf "$P"
mkdir -p "$P"
tar -C "$R" --owner=0 --group=0 -czf "$P/data.tar.gz" ./usr
tar -C "$R/DEBIAN" --owner=0 --group=0 -czf "$P/control.tar.gz" ./control ./postinst
printf '2.0\n' > "$P/debian-binary"
D=build/dist
mkdir -p "$D"
DABS="$(pwd)/build/dist"
rm -f "$DABS/zek-music_${VER}_amd64.deb"
(cd "$P" && ar r "$DABS/zek-music_${VER}_amd64.deb" debian-binary control.tar.gz data.tar.gz)

echo "Pronto: $D/zek-music_${VER}_amd64.deb"
echo "Instalar com:  sudo apt install ./$D/zek-music_${VER}_amd64.deb"