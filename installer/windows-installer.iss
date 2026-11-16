; Parchment — Minimal Zero-Trace Windows Installer (Inno Setup)
; Compiled in GitHub Actions after ./gradlew createReleaseDistributable (ProGuard-shrunk jars)

#define MyAppName "Parchment"
#ifndef MyAppVersion
  #define MyAppVersion "2.0.0"
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
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
SetupIconFile=parchment-icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\composeApp\build\compose\binaries\main-release\app\Parchment\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
; Builds the app's class archive at the installed path (headless, exits within 10 s) so the first launch after an
; install or update starts at full speed. Runs in silent updates too; a failure only leaves that first launch slower.
Filename: "{app}\{#MyAppExeName}"; Parameters: "--cds-warmup"; StatusMsg: "Preparing Parchment for a fast first start..."; Flags: runhidden waituntilterminated
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

; ==============================================================================
; SAFE UNINSTALLATION: Only removes the application binaries in {app},
; NEVER deleting %LOCALAPPDATA%\ParchmentAI so updates/reinstalls keep all user data!
; ==============================================================================
[UninstallDelete]
Type: filesandordirs; Name: "{app}"
