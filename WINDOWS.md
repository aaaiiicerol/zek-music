# Compilar Zek Music para Windows (local, antes do GitHub)

Este guia mostra como compilar a versão **Windows** do Zek Music no seu próprio PC (ou na VM Windows 10 que você já tem) para testar se tudo funciona **antes** de fazer o `git push`. Assim o GitHub Actions não trava com erro.

---

## O que você precisa (no Windows)

| Ferramenta | Para que serve |
|------------|----------------|
| Rust (rustup) | Compila o código fonte |
| Inno Setup 6 | Cria o instalador `.exe` |
| 7-Zip | Compacta a versão portátil |

---

## 1. Preparar o Windows (faça uma vez só)

Na sua VM Windows 10 (ou PC):

### A. Instalar o Rust
1. Abra o navegador dentro do Windows e acesse: https://www.rustup.rs/
2. Baixe e execute o `rustup-init.exe`.
3. Na instalação, escolha a opção **1) Proceed with installation (default)**.
4. Depois de instalar, **feche o PowerShell e abra novamente**.
5. Teste:
   ```powershell
   rustc --version
   cargo --version
   ```

### B. Instalar o Inno Setup 6
1. Baixe em: https://jrsoftware.org/is.php (arquivo `is-6.x.exe`).
2. Instale normalmente. Ele fica em:
   ```
   C:\Program Files (x86)\Inno Setup 6\ISCC.exe
   ```

### C. Instalar o 7-Zip (para a versão portátil)
1. Baixe em: https://www.7-zip.org/
2. Instale.

---

## 2. Baixar o repositório na VM Windows

No navegador do Windows, acesse:

```
https://github.com/aaaiiicerol/zek-music
```

Clique no botão verde **<> Code** → **Download ZIP**.  
Extraia o ZIP em `C:\Users\SeuNome\Documents\zek-music`.

---

## 3. Compilar (use o script pronto)

No PowerShell do Windows, entre na pasta do projeto:

```powershell
cd C:\Users\SeuNome\Documents\zek-music
```

Depois rode:

```powershell
.\compilar_windows.bat
```

O script vai:
1. Compilar o código com `cargo build --release` (gera `target\release\zek-music.exe`)
2. Criar o instalador `dist\ZekMusic-Setup.exe` com o Inno Setup
3. Criar a versão portátil `ZekMusic-Windows-Portable-x.x.x.zip`

Se terminar sem erro, **está pronto para enviar ao GitHub**.

---

## 4. Só então faça o git push

```bash
git add .
git commit -m "compila OK no Windows"
git push origin main
```

Se quiser publicar direto no Releases, crie uma tag:
```bash
git tag v0.6.0
git push origin v0.6.0
```

O workflow `.github/workflows/build.yml` vai rodar no GitHub Actions (Windows) e publicar os arquivos em Releases.

---

## 5. Se der erro no GitHub Actions

Acesse: https://github.com/aaaiiicerol/zek-music/actions

Abra o workflow **Compilar pacotes** → job **windows** e veja o log do passo que falhou. Corrija localmente no Windows com o `compilar_windows.bat`, faça commit e push novamente.

---

## Arquivos envolvidos

| Arquivo | O que faz |
|---------|-----------|
| `Cargo.toml` | Configuração do projeto Rust |
| `src/main.rs` | Código fonte principal |
| `packaging/windows/zek-music.iss` | Script do Inno Setup para criar o instalador |
| `compilar_windows.bat` | Script que roda tudo no Windows (este guia) |
| `WINDOWS.md` | Este guia |
| `.github/workflows/build.yml` | Faz o mesmo no GitHub Actions (não edite a menos que saiba o que está fazendo) |

---

> **Dica:** não tente compilar Windows diretamente do Linux com `cargo build --target x86_64-pc-windows-msvc`. É complexo e exige muitas bibliotecas do Windows. Use a VM Windows 10 que você já tem, ou deixe o GitHub Actions fazer isso para você.
