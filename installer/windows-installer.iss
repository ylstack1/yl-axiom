; Parchment — Minimal Zero-Trace Windows Installer (Inno Setup)
; Compiled in GitHub Actions after cargo build --release -p parchment-desktop

#define MyAppName "Parchment"
#ifndef MyAppVersion
  #define MyAppVersion "3.0.0"
#endif
#ifndef MyCompression
  #define MyCompression "lzma2/max"
#endif
#define MyAppPublisher "Parchment"
#define MyAppExeName "Parchment.exe"

[Setup]
AppId={{8D49C66A-9870-4C97-9499-684F84A8E432}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\build\installer
OutputBaseFilename=Parchment-Windows-x64-Setup
Compression={#MyCompression}
SolidCompression=yes
LZMAUseSeparateProcess=yes
LZMANumBlockThreads=4
TimeStampsInUTC=yes
WizardStyle=modern
SetupIconFile=parchment-icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\parchment-desktop.exe"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

; ==============================================================================
; SAFE UNINSTALLATION: Only removes the application binaries in {app},
; NEVER deleting %LOCALAPPDATA%\ParchmentAI so updates/reinstalls keep all user data!
; ==============================================================================
[UninstallDelete]
Type: filesandordirs; Name: "{app}"
