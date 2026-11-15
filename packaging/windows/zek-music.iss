; Instalador do Zek Music (Inno Setup 6)
; Gerado pelo robô do GitHub: ISCC.exe /DMyAppVersion=0.4.0 packaging\windows\zek-music.iss

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif
#define MyAppName "Zek Music"
#define MyAppExe "zek-music.exe"

[Setup]
AppId={{8800198B-678A-408B-9B7A-E196F6E5FECA}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher=ezekielso
AppPublisherURL=https://github.com/ezekielso/zek-music
AppSupportURL=https://github.com/ezekielso/zek-music/issues
AppUpdatesURL=https://github.com/ezekielso/zek-music/releases
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

[Files]
Source: "..\..\target\release\{#MyAppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExe}"; Description: "{cm:LaunchProgram,{#MyAppName}}"; Flags: nowait postinstall skipifsilent
