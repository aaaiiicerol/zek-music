#!/usr/bin/env bash
# Faz o "Abrir com o Zek Music" funcionar para quem instalou pelo .rpm/.deb
# do outro jeito, ou usa o AppImage / a versão portátil.
# Registra o programa no menu e associa os arquivos de música a ele.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

# onde está o programa (portátil: nesta pasta; instalado: /usr/bin/zek-music)
if [ -x ./zek-music ]; then
  BIN="$(pwd)/zek-music"
elif [ -x /usr/bin/zek-music ]; then
  BIN=/usr/bin/zek-music
else
  echo "Não encontrei o programa. Rode este script de dentro da pasta do Zek Music."
  exit 1
fi

APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor"
mkdir -p "$APPS" "$ICONS/scalable/apps" "$ICONS/512x512/apps"

sed -e "s|^Exec=zek-music|Exec=$BIN|" packaging/linux/zek-music.desktop > "$APPS/zek-music.desktop"
chmod 644 "$APPS/zek-music.desktop"

if [ -f packaging/zek-music.svg ]; then
  cp packaging/zek-music.svg "$ICONS/scalable/apps/zek-music.svg"
fi
[ -f packaging/zek-music.png ] && cp packaging/zek-music.png "$ICONS/512x512/apps/zek-music.png"

update-desktop-database "$APPS" 2>/dev/null || true
xdg-mime default zek-music.desktop audio/mpeg audio/flac audio/ogg audio/wav audio/mp4 2>/dev/null || true
update-mime-database "$HOME/.local/share/mime" 2>/dev/null || true

echo
echo "Pronto! Agora você pode:"
echo "  • clicar duas vezes num .mp3/.flac/.ogg/.wav e ele abre no Zek Music"
echo "  • mandar arquivos para \"Abrir com...\" e escolher o Zek Music"
echo "  • usar o botão direito num arquivo de música → Abrir com → Zek Music"