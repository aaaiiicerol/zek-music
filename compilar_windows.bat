@echo off
chcp 65001 >nul
setlocal enabledelayedexpansion

echo ========================================
echo  Zek Music - Compilador Windows (local)
echo ========================================
echo.

:: Verifica se está na pasta do repositório
if not exist Cargo.toml (
    echo ERRO: Cargo.toml nao encontrado.
    echo     Rode este script dentro da pasta do zek-music.
    pause
    exit /b 1
)

:: ---------------------------------------------------------
:: 1. Rust / cargo
:: ---------------------------------------------------------
echo [1/5] Verificando Rust e Cargo...
rustc --version >nul 2>&1
if %ERRORLEVEL% neq 0 (
    echo ERRO: Rust nao encontrado no PATH.
    echo     Instale em https://www.rustup.rs/
    echo     e reinicie o PowerShell/Prompt depois da instalacao.
    pause
    exit /b 1
)
cargo --version >nul 2>&1
if %ERRORLEVEL% neq 0 (
    echo ERRO: Cargo nao encontrado.
    pause
    exit /b 1
)
echo OK: Rust e Cargo encontrados.

:: ---------------------------------------------------------
:: 2. Compilar com cargo
:: ---------------------------------------------------------
echo [2/5] Compilando com cargo build --release (pode demorar varios minutos)...
cargo build --release
if %ERRORLEVEL% neq 0 (
    echo ERRO: falha na compilacao do Rust.
    pause
    exit /b 1
)
if not exist target\release\zek-music.exe (
    echo ERRO: zek-music.exe nao foi gerado em target\release\.
    pause
    exit /b 1
)
echo OK: zek-music.exe compilado.

:: ---------------------------------------------------------
:: 3. Versao
:: ---------------------------------------------------------
for /f "tokens=* delims=" %%v in ('findstr /r /c:"^version *= *" Cargo.toml') do set "LINHA=%%v"
set "VERSAO=%LINHA:version =%"
set "VERSAO=%VERSAO:"=%"
set "VERSAO=%VERSAO: =%"
if not defined VERSAO set "VERSAO=0.0.0"
echo Versao detectada: %VERSAO%

:: ---------------------------------------------------------
:: 4. Inno Setup
:: ---------------------------------------------------------
echo [3/5] Verificando Inno Setup 6...
set "ISCC=C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
if not exist "%ISCC%" (
    echo ERRO: Inno Setup 6 nao esta instalado.
    echo     Baixe em https://jrsoftware.org/is.php
    pause
    exit /b 1
)
echo OK: Inno Setup encontrado.

:: ---------------------------------------------------------
:: 5. Instalador e portatil
:: ---------------------------------------------------------
echo [4/5] Criando o instalador com Inno Setup...
"%ISCC%" /DMyAppVersion=%VERSAO% packaging\windows\zek-music.iss
if %ERRORLEVEL% neq 0 (
    echo ERRO: falha ao criar o instalador.
    pause
    exit /b 1
)
echo OK: instalador criado em dist\ZekMusic-Setup.exe.

echo [5/5] Criando a versao portatil...
set "PORTDIR=portable\ZekMusic"
if exist "%PORTDIR%" rd /s /q "%PORTDIR%"
mkdir "%PORTDIR%"
copy target\release\zek-music.exe "%PORTDIR%\"
copy LICENSE "%PORTDIR%\"
echo Programa portatil: as configuracoes ficam na pasta 'dados' ao lado dele. > "%PORTDIR%\LEIA-ME.txt"
type nul > "%PORTDIR%\portable.txt"
powershell -NoProfile -Command "Compress-Archive -Path '%PORTDIR%\*' -DestinationPath 'ZekMusic-Windows-Portable-%VERSAO%.zip' -Force"

echo.
echo ========================================
echo  SUCESSO!
echo ========================================
echo Instalador: dist\ZekMusic-Setup.exe
echo Portatil  : ZekMusic-Windows-Portable-%VERSAO%.zip
echo.
echo Agora pode fazer git push com seguranca.
pause
