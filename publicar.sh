#!/usr/bin/env bash
# Publica o Zek Music no GitHub: cria o repositório, envia os arquivos
# e dispara a compilação automática do AppImage (que vai para "Releases").
set -e
cd "$(dirname "$0")"

REPO="zek-music"
VERSAO="v0.3.0"
DESC="Player de músicas leve para Linux onde cada pasta vira uma playlist. Feito em Rust."

echo "=== Publicando o Zek Music no GitHub ==="

# 1) GitHub CLI instalado?
if ! command -v gh >/dev/null 2>&1; then
  echo "O GitHub CLI (gh) não está instalado."
  echo "Instale com:  sudo dnf install gh   e rode ./publicar.sh de novo."
  exit 1
fi

# 2) Login (só pede se ainda não estiver logado)
if ! gh auth status >/dev/null 2>&1; then
  echo "Vamos fazer login no GitHub..."
  gh auth login
fi
gh auth setup-git >/dev/null 2>&1 || true
USUARIO=$(gh api user --jq .login)
echo "Logado como: $USUARIO"

# 3) Identidade do Git (usa o e-mail privado do GitHub, para não expor o seu)
if [ -z "$(git config --global user.name || true)" ]; then
  git config --global user.name "$USUARIO"
fi
if [ -z "$(git config --global user.email || true)" ]; then
  ID=$(gh api user --jq .id)
  EMAIL="${ID}+${USUARIO}@users.noreply.github.com"
  git config --global user.email "$EMAIL"
  echo "E-mail configurado (privado do GitHub): $EMAIL"
fi

# 4) Coloca o seu usuário no README
sed -i "s/SEU_USUARIO/$USUARIO/g" README.md

# 5) Prepara o repositório local
[ -d .git ] || git init -q
git branch -M main
git add .
git commit -q -m "Zek Music $VERSAO" || echo "(nada novo para registrar)"

# 6) Cria no GitHub e envia (ou só envia, se já existir)
if gh repo view "$USUARIO/$REPO" >/dev/null 2>&1; then
  echo "O repositório já existe no GitHub. Enviando as alterações..."
  git remote get-url origin >/dev/null 2>&1 || git remote add origin "https://github.com/$USUARIO/$REPO.git"
  git push -u origin main
else
  gh repo create "$REPO" --public --description "$DESC" --source=. --remote=origin --push
fi

# 7) Tópicos do repositório (ajudam as pessoas a encontrar o projeto)
gh repo edit "$USUARIO/$REPO" --add-topic rust,music-player,linux,appimage,egui >/dev/null 2>&1 || true

# 8) Tag de versão: é ela que dispara a compilação do AppImage
if git rev-parse "$VERSAO" >/dev/null 2>&1; then
  echo "A tag $VERSAO já existe localmente."
else
  git tag "$VERSAO"
fi
git push origin "$VERSAO" || true

echo
echo "=== Pronto! ==="
echo "Página do projeto:   https://github.com/$USUARIO/$REPO"
echo "Acompanhar a build:  https://github.com/$USUARIO/$REPO/actions"
echo "Download (depois):   https://github.com/$USUARIO/$REPO/releases/latest"
echo
echo "A compilação leva uns 10 a 15 minutos. Quando a bolinha ficar verde,"
echo "o AppImage aparece em Releases."
