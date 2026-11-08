; Setup wizard for CursorPie on Windows, built with Inno Setup 6.3 or later (https://jrsoftware.org/isinfo.php).
;
;   scripts\package-windows.ps1                 builds the program, the zip and this installer
;   ISCC.exe /DAppVersion=0.4.1 installer\cursorpie.iss     builds only the installer (after `cargo build --release`)
;
; The result is target\bundle\CursorPie-<version>-setup.exe. It installs for the current user only
; (no administrator rights, no UAC prompt) into %LOCALAPPDATA%\Programs\CursorPie, the same folder
; the program uses when it installs itself from the zip, so the two never leave two copies behind.
; Settings in %APPDATA%\CursorPie are never touched, not even by the uninstaller.
;
; Keep AppName, the publisher and the URLs in step with src/about.rs when rebranding.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#define AppName "CursorPie"
#define AppPublisher "Sebastvnd"
#define AppUrl "https://github.com/sebastvnd/cursorpie"
#define AppExe "cursorpie.exe"

[Setup]
; Never change AppId: Windows uses it to recognize an earlier install and update it in place.
AppId={{6F0C3E52-8B1D-4C7A-9E34-5D2A7B91C0F8}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}/issues
AppUpdatesURL={#AppUrl}/releases
VersionInfoVersion={#AppVersion}
VersionInfoCompany={#AppPublisher}
VersionInfoDescription={#AppName} Setup
VersionInfoProductName={#AppName}
VersionInfoProductVersion={#AppVersion}
; {autopf} is %LOCALAPPDATA%\Programs when the installer runs without administrator rights
PrivilegesRequired=lowest
DefaultDirName={autopf}\{#AppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
LicenseFile=..\LICENSE
OutputDir=..\target\bundle
OutputBaseFilename={#AppName}-{#AppVersion}-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupIconFile=..\assets\app-icon.ico
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\{#AppExe}
; the program is a tray app with no window, so Setup cannot ask it to close: see [Code]
CloseApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "startup"; Description: "Start {#AppName} when I sign in to Windows"; GroupDescription: "Startup:"

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"

[Registry]
; The same entry the program writes itself (see src/platform/windows/login_item.rs): the path in quotes.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "{#AppName}"; ValueData: """{app}\{#AppExe}"" --autostart"; Tasks: startup; Flags: uninsdeletevalue
; An explicit "enabled" flag, so an old "disabled" flag left by Task Manager does not win.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"; ValueType: binary; ValueName: "{#AppName}"; ValueData: "02 00 00 00 00 00 00 00 00 00 00 00"; Tasks: startup; Flags: uninsdeletevalue

[Run]
Filename: "{app}\{#AppExe}"; Description: "Start {#AppName} now"; Flags: nowait postinstall skipifsilent
; The update that CursorPie installs by itself (src/update/install.rs) runs this Setup silently with
; /RESTARTAPP=1 and expects the new version to start when it is done. A silent run without that
; switch (a script, a deployment tool) starts nothing, as before.
Filename: "{app}\{#AppExe}"; Flags: nowait runasoriginaluser skipifnotsilent; Check: RestartAfterUpdate

[Code]
// Ends every running copy of the program. It is a tray app without a window, so Setup cannot
// close it politely, and a running .exe cannot be replaced or deleted.
procedure StopCursorPie;
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM {#AppExe}', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

function RestartAfterUpdate: Boolean;
begin
  Result := ExpandConstant('{param:RESTARTAPP|0}') = '1';
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopCursorPie;
  Result := '';
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    // The program can install itself from the zip and then has its own "Apps & features" entry.
    // The wizard now owns the install, so that older entry would be a second, stale one.
    RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#AppName}');
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    StopCursorPie;
end;
