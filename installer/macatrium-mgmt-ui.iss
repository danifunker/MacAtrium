; Inno Setup script for the MacAtrium Manager (Windows, per-user install).
;
; Produces a per-user Setup.exe that installs the egui management GUI **and the
; atrium CLI** into %LocalAppData% (no admin needed) — both land in the same
; install dir. CI stages both exes into one SourceDir before invoking this.
;
; Build (CI passes these /D defines):
;   iscc /DMyAppVersion=2026-06-22-07-30 /DSourceDir=path\to\stage \
;        /DAssetsDir=path\to\assets\icons /FMacAtrium-Manager-Setup installer\macatrium-mgmt-ui.iss

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0-dev"
#endif

; Directory containing the built macatrium-mgmt-ui.exe.
#ifndef SourceDir
  #define SourceDir "..\tools\macatrium-mgmt-ui\target\release"
#endif

; Directory containing macatrium.ico (assets live outside the build dir).
#ifndef AssetsDir
  #define AssetsDir "..\assets\icons"
#endif

; The atrium CLI installs in the same dir as the GUI. By default it lives
; alongside the GUI exe (CI stages both into one SourceDir); override CliSourceDir
; to point elsewhere. skipifsourcedoesntexist keeps GUI-only local builds working.
#ifndef CliSourceDir
  #define CliSourceDir SourceDir
#endif
#define CliExeName "atrium.exe"

#define MyAppName "MacAtrium Manager"
#define MyAppPublisher "Dani Sarfati"
#define MyAppURL "https://github.com/danifunker/MacAtrium"
#define MyAppExeName "macatrium-mgmt-ui.exe"

[Setup]
; Stable AppId so upgrades replace in place; do not change this GUID.
AppId={{B7E2D9C4-1A56-4F3B-8E2D-9C4A1B56F30A}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}/releases
; Per-user install: no elevation, installs under %LocalAppData%.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
DefaultDirName={localappdata}\Programs\MacAtrium Manager
DisableProgramGroupPage=yes
DefaultGroupName={#MyAppName}
DisableDirPage=no
AllowNoIcons=yes
UninstallDisplayIcon={app}\{#MyAppExeName}
OutputBaseFilename=MacAtrium-Manager-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#SourceDir}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
; The atrium CLI, shipped in the same package — skip cleanly for GUI-only builds.
Source: "{#CliSourceDir}\{#CliExeName}"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
; Icon is optional — skip cleanly if assets aren't present in this build.
Source: "{#AssetsDir}\macatrium.ico"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{userdesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent
