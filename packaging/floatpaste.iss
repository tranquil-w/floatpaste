; FloatPaste installer script (Inno Setup 6)
; CI build: iscc /DAppVersion=<version> packaging/floatpaste.iss
; Prerequisite: cargo build --release -p floatpaste-native (produces target/release/floatpaste.exe)

#ifndef AppVersion
  #error Pass the version via /DAppVersion, e.g. iscc /DAppVersion=0.9.0 packaging/floatpaste.iss
#endif

#define AppName "FloatPaste"
#define AppExeName "floatpaste.exe"

[Setup]
AppId={{6C4F0E9A-2B7D-4E3A-9F81-A05C8D1B2E47}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppName}
; Per-user install (no admin prompt), matching the previous NSIS layout
DefaultDirName={localappdata}\Programs\{#AppName}
PrivilegesRequired=lowest
OutputDir=output
OutputBaseFilename=FloatPaste_{#AppVersion}_x64-setup
SetupIconFile=..\crates\floatpaste-native\assets\icon.ico
UninstallDisplayIcon={app}\{#AppExeName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupLogging=yes
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Additional tasks:"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExeName}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent

[Code]
// The app lives in the tray; kill it before install/uninstall so the exe is not locked.
procedure CloseApp;
var
  ResultCode: Integer;
begin
  Exec('taskkill.exe', '/IM floatpaste.exe /F', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

function InitializeSetup(): Boolean;
begin
  CloseApp;
  Result := True;
end;

function InitializeUninstall(): Boolean;
begin
  CloseApp;
  Result := True;
end;
