#!/usr/bin/env bash
# Gera tudo de uma vez: .deb, .rpm, AppImage e versão portátil.
set -euo pipefail
cd "$(dirname "$0")"
echo "=== Gerando todos os pacotes do Zek Music ==="
bash ./build-deb.sh
bash ./build-rpm.sh
bash ./build-appimage.sh
bash ./build-portatil.sh
mkdir -p build/dist
( cd build/dist && sha256sum -- * > SHA256SUMS.txt 2>/dev/null || true )
echo
echo "=== Pronto! Arquivos em build/dist: ==="
ls -lh build/dist
