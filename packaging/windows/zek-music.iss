; Instalador do Zek Music (Inno Setup 6)
; Gerado pelo robô do GitHub: ISCC.exe /DMyAppVersion=0.4.0 packaging\windows\zek-music.iss

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif
#define MyAppName "Zek Music"
#define MyAppExe "zek-music.exe"
#define MyAppProgID "ZekMusic"

[Setup]
AppId={{8800198B-678A-408B-9B7A-E196F6E5FECA}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher=aaaiiicerol
AppPublisherURL=https://github.com/aaaiiicerol/zek-music
AppSupportURL=https://github.com/aaaiiicerol/zek-music/issues
AppUpdatesURL=https://github.com/aaaiiicerol/zek-music/releases
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
LicenseFile=..\..\LICENSE
OutputDir=..\..\dist
OutputBaseFilename=ZekMusic-Setup
SetupIconFile=..\zek-music.ico
UninstallDisplayIcon={app}\{#MyAppExe}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Instala só para o seu usuário (sem pedir senha de administrador); o assistente deixa escolher
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "associar"; Description: "Associar arquivos de musica (.mp3, .flac, .ogg, .wav, .m4a, .aac) ao Zek Music"; GroupDescription: "Ao instalar"; Flags: checkedonce

[Files]
Source: "..\..\target\release\{#MyAppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"; Tasks: desktopicon

[Registry]
; ── associação dos tipos de arquivo (aplicativo + "abrir com") ──
Root: HKCU; Subkey: "Software\Classes\.mp3\OpenWithProgids"; ValueType: string; ValueName: "{#MyAppProgID}"; ValueData: ""; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\.flac\OpenWithProgids"; ValueType: string; ValueName: "{#MyAppProgID}"; ValueData: ""; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\.ogg\OpenWithProgids"; ValueType: string; ValueName: "{#MyAppProgID}"; ValueData: ""; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\.oga\OpenWithProgids"; ValueType: string; ValueName: "{#MyAppProgID}"; ValueData: ""; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\.wav\OpenWithProgids"; ValueType: string; ValueName: "{#MyAppProgID}"; ValueData: ""; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\.m4a\OpenWithProgids"; ValueType: string; ValueName: "{#MyAppProgID}"; ValueData: ""; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\.aac\OpenWithProgids"; ValueType: string; ValueName: "{#MyAppProgID}"; ValueData: ""; Tasks: associar

Root: HKCU; Subkey: "Software\Classes\{#MyAppProgID}"; ValueType: string; ValueData: "Arquivo de música"; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\{#MyAppProgID}"; ValueName: "AppUserModelID"; ValueType: string; ValueData: "{#MyAppProgID}"; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\{#MyAppProgID}\DefaultIcon"; ValueType: string; ValueData: "{app}\{#MyAppExe},0"; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\{#MyAppProgID}\shell\open"; ValueType: string; ValueData: "Tocar no Zek Music"; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\{#MyAppProgID}\shell\open\command"; ValueType: string; ValueData: """{app}\{#MyAppExe}"" ""%1"""; Tasks: associar

; "Abrir com" lista o programa
Root: HKCU; Subkey: "Software\Classes\Applications\{#MyAppExe}"; ValueType: string; ValueData: "{#MyAppName}"; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\Applications\{#MyAppExe}\SupportedTypes"; ValueType: string; ValueName: ".mp3"; ValueData: ""; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\Applications\{#MyAppExe}\SupportedTypes"; ValueType: string; ValueName: ".flac"; ValueData: ""; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\Applications\{#MyAppExe}\SupportedTypes"; ValueType: string; ValueName: ".ogg"; ValueData: ""; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\Applications\{#MyAppExe}\SupportedTypes"; ValueType: string; ValueName: ".wav"; ValueData: ""; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\Applications\{#MyAppExe}\SupportedTypes"; ValueType: string; ValueName: ".m4a"; ValueData: ""; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\Applications\{#MyAppExe}\SupportedTypes"; ValueType: string; ValueName: ".aac"; ValueData: ""; Tasks: associar
Root: HKCU; Subkey: "Software\Classes\Applications\{#MyAppExe}\shell\open\command"; ValueType: string; ValueData: """{app}\{#MyAppExe}"" ""%1"""; Tasks: associar

; notifica o Windows que as associações mudaram
[Run]
Filename: "ie4uinit.exe"; Parameters: "-ClearIconCache"; Tasks: associar; Flags: runhidden; RunOnceId: "IconCache"
Filename: "{app}\{#MyAppExe}"; Description: "{cm:LaunchProgram,{#MyAppName}}"; Flags: nowait postinstall skipifsilent