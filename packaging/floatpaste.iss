; FloatPaste 安装包脚本（Inno Setup 6.3+，UTF-8 带 BOM）
; CI 构建：iscc /DAppVersion=<版本号> packaging/floatpaste.iss
; 前置条件：cargo build --release -p floatpaste-native（产出 target/release/floatpaste.exe）

#ifndef AppVersion
  #error 请通过 /DAppVersion 传入版本号，例如 iscc /DAppVersion=0.9.0 packaging/floatpaste.iss
#endif

#define AppName "FloatPaste"
#define AppExeName "floatpaste.exe"
#define SetupId "6C4F0E9A-2B7D-4E3A-9F81-A05C8D1B2E47"
#define UninstKeyName "{" + SetupId + "}_is1"

[Setup]
AppId={{6C4F0E9A-2B7D-4E3A-9F81-A05C8D1B2E47}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppName}
; 默认安装到 C:\Program Files\FloatPaste，需要管理员权限
DefaultDirName={autopf}\{#AppName}
PrivilegesRequired=admin
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

[Languages]
Name: "chs"; MessagesFile: "ChineseSimplified.isl"

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式(&D)"; GroupDescription: "附加任务："; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExeName}"; Description: "启动 {#AppName}"; Flags: nowait postinstall skipifsilent

[Code]
const
  UninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall';
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  RunValueName = 'FloatPaste';
  ExplorerAdvancedKey = 'Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced';
  DisabledHotkeysValue = 'DisabledHotkeys';

var
  g_RunMigrate: Boolean;   // 检测到指向旧安装位置的自启动条目
  g_RunArgs: String;       // 自启动条目的启动参数（如 --silent）

// 结束运行中的实例，避免文件被占用
procedure CloseApp;
var
  ResultCode: Integer;
begin
  Exec('taskkill.exe', '/IM {#AppExeName} /F', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

// 尝试卸载一个旧版卸载条目；跳过当前安装器的条目
function TryUninstallEntry(RootKey: Integer; SubKey: String): Boolean;
var
  DisplayName, Cmd, Guid: String;
  P: Integer;
  ResultCode: Integer;
begin
  Result := False;
  if CompareText(SubKey, '{#UninstKeyName}') = 0 then
    Exit;
  if not RegQueryStringValue(RootKey, UninstallKey + '\' + SubKey, 'DisplayName', DisplayName) then
    Exit;
  if CompareText(DisplayName, '{#AppName}') <> 0 then
    Exit;
  if not RegQueryStringValue(RootKey, UninstallKey + '\' + SubKey, 'UninstallString', Cmd) then
    Exit;

  if Pos('msiexec', LowerCase(Cmd)) > 0 then begin
    // MSI 旧版：提取 {GUID} 后静默卸载
    P := Pos('{', Cmd);
    if (P > 0) and (Pos('}', Cmd) > P) then begin
      Guid := Copy(Cmd, P, Pos('}', Cmd) - P + 1);
      Exec('msiexec.exe', '/x' + Guid + ' /qn /norestart', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
      Result := True;
    end;
  end else if Pos('uninstall', LowerCase(Cmd)) > 0 then begin
    // NSIS 旧版：取卸载器路径后静默执行
    if Cmd[1] = '"' then
      Cmd := Copy(Cmd, 2, Pos('"', Copy(Cmd, 2, Length(Cmd))) - 1)
    else begin
      P := Pos(' ', Cmd);
      if P > 0 then
        Cmd := Copy(Cmd, 1, P - 1);
    end;
    if FileExists(Cmd) then begin
      Exec(Cmd, '/S', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
      Result := True;
    end;
  end;
end;

procedure UninstallOldVersions;
var
  Keys: TArrayOfString;
  I: Integer;
  Count: Integer;
begin
  Count := 0;
  if RegGetSubkeyNames(HKEY_CURRENT_USER, UninstallKey, Keys) then
    for I := 0 to GetArrayLength(Keys) - 1 do
      if TryUninstallEntry(HKEY_CURRENT_USER, Keys[I]) then
        Count := Count + 1;
  if RegGetSubkeyNames(HKEY_LOCAL_MACHINE_64, UninstallKey, Keys) then
    for I := 0 to GetArrayLength(Keys) - 1 do
      if TryUninstallEntry(HKEY_LOCAL_MACHINE_64, Keys[I]) then
        Count := Count + 1;
  if RegGetSubkeyNames(HKEY_LOCAL_MACHINE_32, UninstallKey, Keys) then
    for I := 0 to GetArrayLength(Keys) - 1 do
      if TryUninstallEntry(HKEY_LOCAL_MACHINE_32, Keys[I]) then
        Count := Count + 1;
  if (Count > 0) and (not WizardSilent()) then
    MsgBox('检测到旧版本，已自动卸载。', mbInformation, MB_OK);
end;

// 记录旧的自启动条目（仅当指向旧版安装位置时才迁移）
procedure CaptureRunEntry;
var
  OldVal: String;
  P: Integer;
begin
  g_RunMigrate := False;
  g_RunArgs := '';
  if not RegQueryStringValue(HKEY_CURRENT_USER, RunKey, RunValueName, OldVal) then
    Exit;
  OldVal := Trim(OldVal);
  if OldVal = '' then
    Exit;
  if Pos(LowerCase(ExpandConstant('{localappdata}')), LowerCase(OldVal)) = 0 then
    Exit;

  if OldVal[1] = '"' then begin
    P := Pos('"', Copy(OldVal, 2, Length(OldVal)));
    if P = 0 then
      Exit;
    g_RunArgs := Trim(Copy(OldVal, P + 2, Length(OldVal)));
  end else begin
    P := Pos(' ', OldVal);
    if P > 0 then
      g_RunArgs := Trim(Copy(OldVal, P + 1, Length(OldVal)));
  end;
  g_RunMigrate := True;
end;

// 把自启动条目改写到新安装位置，保留原启动参数
procedure MigrateRunEntry;
var
  NewCmd: String;
begin
  if not g_RunMigrate then
    Exit;
  NewCmd := AddQuotes(ExpandConstant('{app}\{#AppExeName}'));
  if g_RunArgs <> '' then
    NewCmd := NewCmd + ' ' + g_RunArgs;
  RegWriteStringValue(HKEY_CURRENT_USER, RunKey, RunValueName, NewCmd);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  CloseApp;
  CaptureRunEntry;
  UninstallOldVersions;
  MigrateRunEntry;
end;

function InitializeUninstall(): Boolean;
begin
  CloseApp;
  Result := True;
end;

// 卸载时清除 Win+V 接管在 DisabledHotkeys 里写的 V 字母（其他字母
// 原样保留，删空则整值删除），否则系统 Win+V 在卸载后永久失效。
// 清理后需重启资源管理器系统才重新持有该组合，向用户说明
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Value, Kept: String;
  I: Integer;
begin
  if CurUninstallStep <> usPostUninstall then
    Exit;
  if not RegQueryStringValue(HKEY_CURRENT_USER, ExplorerAdvancedKey, DisabledHotkeysValue, Value) then
    Exit;
  Kept := '';
  for I := 1 to Length(Value) do
    if (Value[I] <> 'V') and (Value[I] <> 'v') then
      Kept := Kept + Value[I];
  if Kept = '' then
    RegDeleteValue(HKEY_CURRENT_USER, ExplorerAdvancedKey, DisabledHotkeysValue)
  else
    RegWriteStringValue(HKEY_CURRENT_USER, ExplorerAdvancedKey, DisabledHotkeysValue, Kept);
  if not WizardSilent() then
    MsgBox('已恢复系统 Win+V 剪贴板历史热键。' + #13#10 +
      '若未自动生效，请重启资源管理器或注销后重新登录。', mbInformation, MB_OK);
end;
