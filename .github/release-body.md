## 🎵 Zek Music

Player de músicas leve onde **cada pasta vira uma playlist**. Feito em Rust, para Linux e Windows.

### 📥 Baixar

| Sistema | Arquivo | Para quem |
|---|---|---|
| 🐧 Debian/Ubuntu/Mint | `zek-music_0.6.0_amd64.deb` | instala no sistema, com o "Abrir com" já configurado |
| 🐧 Fedora/openSUSE | `zek-music-0.6.0-1.x86_64.rpm` | instala no sistema, com o "Abrir com" já configurado |
| 🐧 Qualquer distro | `ZekMusic-x86_64.AppImage` | um arquivo só: `chmod +x` e execute |
| 🐧 Pendrive | `ZekMusic-Portatil-0.6.0-linux-x86_64.tar.gz` | portátil: guarda tudo na própria pasta |
| 🪟 Windows | `ZekMusic-Setup.exe` | instalador (recomendado): atalho no Menu Iniciar e desinstalador |
| 🪟 Windows | `ZekMusic-Windows-Portable-0.6.0.zip` | portátil: extraia e abra, sem instalar |

### ⚠️ Windows: aviso do SmartScreen
Como o programa ainda não tem assinatura digital paga, o Windows pode mostrar *"O Windows protegeu o computador"*.
Clique em **Mais informações → Executar assim mesmo**. O código é aberto e pode ser conferido neste repositório.

### 🐧 Linux — instalação manual

```bash
# .deb / .rpm
sudo apt install ./zek-music_0.6.0_amd64.deb
sudo dnf install ./zek-music-0.6.0-1.x86_64.rpm

# AppImage
chmod +x ZekMusic-x86_64.AppImage && ./ZekMusic-x86_64.AppImage

# portátil (tem um script para associar os arquivos de música)
tar xzf ZekMusic-Portatil-0.6.0-linux-x86_64.tar.gz && cd zek-music && ./instalar-associacoes.sh
```
No Fedora, se o AppImage reclamar de FUSE: `sudo dnf install fuse-libs`.

---

### ✨ O que há de novo nesta versão
- 🛡️ pasta desmontada não apaga mais as playlists (avisa "pasta indisponível")
- 🔢 ordem natural das faixas e pelo número da etiqueta
- ⏯️ continua de onde parou (música, posição, volume, aleatório, repetir)
- 🩺 registro de erros (`zek-music.log`), visível em Ajustes
- 🖼️ capa decodificada em segundo plano (não engasga ao trocar de música)
- ✏️ arrastar playlists para reordenar
- 🎨 cor de destaque escolhível em Ajustes
- 🔔 aviso de nova versão consultando o GitHub (1x ao dia)
- 📂 "Abrir com o Zek Music": duplo clique num .mp3/.flac/.ogg/.wav
- 📦 pacotes .deb e .rpm além do AppImage e do Windows