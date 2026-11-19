#!/usr/bin/env bash
# Gera a versão portátil do Linux: uma pasta que você pode levar no pendrive.
# As configurações ficam dentro da própria pasta (graças ao "portable.txt").
set -euo pipefail
cd "$(dirname "$0")"

VER=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
echo "=== Zek Music $VER — versão portátil (Linux) ==="

cargo build --release

R=build/portavel/zek-music
rm -rf build/portavel
mkdir -p "$R"
cp target/release/zek-music "$R/zek-music"
cp packaging/zek-music.svg "$R/zek-music.svg" 2>/dev/null || cp packaging/zek-music.png "$R/zek-music.png"
: > "$R/portable.txt"          # marca: tudo fica na pasta "dados"
cat > "$R/LEIA-ME.txt" <<EOF
Zek Music $VER — versão portátil (Linux)

Como usar:
  1. dê permissão de execução:  chmod +x zek-music
  2. clique duas vezes no "zek-music" (ou rode:  ./zek-music)

Tudo (playlists, volume, registro de erros) fica guardado na pasta "dados",
criada aqui dentro. Levar a pasta para outro computário leva o programa junto.

Para associar arquivos .mp3 ao programa (duplo clique abre no Zek Music),
rode:  ./instalar-associacoes.sh
EOF
cp instalar-associacoes.sh "$R/" 2>/dev/null || true

mkdir -p build/dist
tar -C build/portavel -czf "build/dist/ZekMusic-Portatil-$VER-linux-x86_64.tar.gz" zek-music
echo "Pronto: build/dist/ZekMusic-Portatil-$VER-linux-x86_64.tar.gz"