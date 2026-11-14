#!/usr/bin/env bash
# Cria o seu perfil do GitHub (repositório com o mesmo nome do usuário).
set -e

if ! command -v gh >/dev/null 2>&1; then
  echo "Instale o GitHub CLI primeiro:  sudo dnf install gh"; exit 1
fi
if ! gh auth status >/dev/null 2>&1; then gh auth login; fi
gh auth setup-git >/dev/null 2>&1 || true
USUARIO=$(gh api user --jq .login)

if gh repo view "$USUARIO/$USUARIO" >/dev/null 2>&1; then
  echo "Você JÁ tem um perfil: https://github.com/$USUARIO"
  echo "Para adicionar o Zek Music, edite o README (ícone de lápis) e acrescente na tabela de projetos:"
  echo
  echo "| 🎵 Zek Music | Player de músicas para Linux onde cada pasta vira uma playlist | [Ver projeto](https://github.com/$USUARIO/zek-music) |"
  exit 0
fi

if [ -z "$(git config --global user.name || true)" ]; then git config --global user.name "$USUARIO"; fi
if [ -z "$(git config --global user.email || true)" ]; then
  git config --global user.email "$(gh api user --jq .id)+${USUARIO}@users.noreply.github.com"
fi

read -rp "Seu nome: " NOME
read -rp "Nome do seu canal no YouTube: " CANAL
read -rp "Link do canal no YouTube: " YT

TMP=$(mktemp -d)
cd "$TMP"
cat > README.md << MD
# Olá, eu sou $NOME 👋

🎮 Faço vídeos sobre **games e Linux** no canal **$CANAL**
🐧 Crio projetos open source para quem joga e usa Linux

[![YouTube](https://img.shields.io/badge/YouTube-FF0000?style=for-the-badge&logo=youtube&logoColor=white)]($YT)
![Linux](https://img.shields.io/badge/Linux-3b82f6?style=for-the-badge&logo=linux&logoColor=white)

## 🚀 Projetos

| Projeto | Descrição | Link |
|---|---|---|
| 🎵 Zek Music | Player de músicas para Linux onde cada pasta vira uma playlist | [Ver projeto](https://github.com/$USUARIO/zek-music) |
| 💡 Luz do Teclado | Controle da luz do teclado no Linux | [Ver projeto](https://github.com/$USUARIO/luz-teclado) |

## 📺 Canal

Quer ver os projetos em ação? Passa lá no canal: **[$CANAL]($YT)**
MD

git init -q
git branch -M main
git add .
git commit -q -m "Perfil"
gh repo create "$USUARIO" --public --description "Meu perfil" --source=. --remote=origin --push

echo
echo "Pronto! Seu perfil: https://github.com/$USUARIO"
