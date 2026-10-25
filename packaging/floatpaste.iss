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
MinVersion=10.0
LicenseFile=license.txt
OutputDir=output
OutputBaseFilename=FloatPaste_{#AppVersion}_x64-setup
SetupIconFile=..\crates\floatpaste-native\assets\icon.ico
UninstallDisplayIcon={app}\{#AppExeName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupLogging=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes
CloseApplications=yes
CloseApplicationsFilter={#AppExeName}
RestartApplications=no

[Languages]
Name: "chs"; MessagesFile: "ChineseSimplified.isl"

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式(&D)"; GroupDescription: "附加任务："; Flags: unchecked
; 仅在检测到旧版卸载条目时显示；默认勾选，静默安装沿用勾选值自动卸载
Name: "uninstallold"; Description: "卸载检测到的旧版本(&U)"; GroupDescription: "附加任务："; Check: ShouldOfferUninstallOld

[Files]
Source: "..\target\release\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExeName}"; Description: "启动 {#AppName}"; Flags: nowait postinstall skipifsilent runasoriginaluser

[Code]
const
  UninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall';
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  RunValueName = 'FloatPaste';
  ExplorerAdvancedKey = 'Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced';
  DisabledHotkeysValue = 'DisabledHotkeys';

type
  TOldVersion = record
    RootKey: Integer;
    SubKey: String;
    ExePath: String;
  end;

var
  g_OldVersions: array of TOldVersion;
  g_PreviousExe: String;
  g_RunCaptured, g_RunMigrate: Boolean;
  g_RestartRequired: Boolean;
  g_RunValue, g_RunArgs: String;
  g_RemoveUserData: Boolean;
  g_UninstallDataDir: String;

// 注册表中的命令可能带引号，也可能是未加引号且含空格的 exe 路径。
function SplitExeCommand(Command: String; var ExePath, Args: String): Boolean;
var
  P: Integer;
begin
  Result := False;
  ExePath := '';
  Args := '';
  Command := Trim(Command);
  if Command = '' then
    Exit;
  if Command[1] = '"' then begin
    P := Pos('"', Copy(Command, 2, Length(Command)));
    if P = 0 then
      Exit;
    ExePath := Copy(Command, 2, P - 1);
    Args := Trim(Copy(Command, P + 2, Length(Command)));
  end else begin
    P := Pos('.exe', LowerCase(Command));
    if P = 0 then
      Exit;
    ExePath := Copy(Command, 1, P + 3);
    Args := Trim(Copy(Command, P + 4, Length(Command)));
  end;
  Result := CompareText(ExtractFileExt(ExePath), '.exe') = 0;
end;

// Inno 无内建环境变量写入，声明 Kernel32 以向迁移脚本传 Unicode 任务名与路径
procedure SetTaskEnv(Name: String; Value: String);
  external 'SetEnvironmentVariableW@Kernel32.dll stdcall';

function IsOldEntry(RootKey: Integer; SubKey: String): Boolean;
var
  DisplayName: String;
begin
  Result := False;
  if CompareText(SubKey, '{#UninstKeyName}') = 0 then
    Exit;
  if RegQueryStringValue(RootKey, UninstallKey + '\' + SubKey, 'DisplayName', DisplayName) then
    Result := CompareText(DisplayName, '{#AppName}') = 0;
end;

// 保存旧路径后再卸载；自启迁移只接受这些已知安装位置。
procedure FindOldVersions(RootKey: Integer);
var
  Keys: TArrayOfString;
  I, N: Integer;
  Location, Command, ExePath, Args: String;
begin
  if not RegGetSubkeyNames(RootKey, UninstallKey, Keys) then
    Exit;
  for I := 0 to GetArrayLength(Keys) - 1 do
    if IsOldEntry(RootKey, Keys[I]) then begin
      N := GetArrayLength(g_OldVersions);
      SetArrayLength(g_OldVersions, N + 1);
      g_OldVersions[N].RootKey := RootKey;
      g_OldVersions[N].SubKey := Keys[I];
      if RegQueryStringValue(RootKey, UninstallKey + '\' + Keys[I], 'InstallLocation', Location) and
         (Trim(Location) <> '') then
        g_OldVersions[N].ExePath := AddBackslash(RemoveQuotes(Location)) + '{#AppExeName}'
      else if RegQueryStringValue(RootKey, UninstallKey + '\' + Keys[I], 'UninstallString', Command) then
        if SplitExeCommand(Command, ExePath, Args) and
           (CompareText(ExtractFileName(ExePath), 'uninstall.exe') = 0) then
          g_OldVersions[N].ExePath := AddBackslash(ExtractFileDir(ExePath)) + '{#AppExeName}';
    end;
end;

function ShouldOfferUninstallOld: Boolean;
begin
  Result := GetArrayLength(g_OldVersions) > 0;
end;

function InitializeSetup(): Boolean;
var
  Location: String;
begin
  if RegQueryStringValue(HKEY_LOCAL_MACHINE_64, UninstallKey + '\{#UninstKeyName}',
       'InstallLocation', Location) and (Trim(Location) <> '') then
    g_PreviousExe := AddBackslash(Location) + '{#AppExeName}';
  FindOldVersions(HKEY_CURRENT_USER_64);
  FindOldVersions(HKEY_CURRENT_USER_32);
  FindOldVersions(HKEY_LOCAL_MACHINE_64);
  FindOldVersions(HKEY_LOCAL_MACHINE_32);
  Result := True;
end;

function IsUpgradeExecutable(ExePath: String): Boolean;
var
  I: Integer;
begin
  Result := CompareText(ExePath, ExpandConstant('{app}\{#AppExeName}')) = 0;
  if (g_PreviousExe <> '') and (CompareText(ExePath, g_PreviousExe) = 0) then
    Result := True;
  for I := 0 to GetArrayLength(g_OldVersions) - 1 do
    if (g_OldVersions[I].ExePath <> '') and
       (CompareText(ExePath, g_OldVersions[I].ExePath) = 0) then
      Result := True;
end;

// PrepareToInstall 可能因重试而多次调用，保留首次捕获的 Run 项。
procedure CaptureRunEntry;
var
  ExePath: String;
begin
  if g_RunCaptured then
    Exit;
  g_RunCaptured := True;
  if RegQueryStringValue(HKEY_CURRENT_USER, RunKey, RunValueName, g_RunValue) then
    if SplitExeCommand(g_RunValue, ExePath, g_RunArgs) then
      g_RunMigrate := IsUpgradeExecutable(ExePath);
end;

function TryUninstallEntry(Entry: TOldVersion): Boolean;
var
  Command, ExePath, Args, ProductCode: String;
  P, Attempt, ResultCode: Integer;
  IsMsi: Boolean;
begin
  Result := True;
  // 前一轮卸载或另一注册表视图可能已移除此条目。
  if not IsOldEntry(Entry.RootKey, Entry.SubKey) then
    Exit;
  Result := False;
  if not RegQueryStringValue(Entry.RootKey, UninstallKey + '\' + Entry.SubKey,
       'UninstallString', Command) then
    Exit;
  if not SplitExeCommand(Command, ExePath, Args) then
    Exit;
  IsMsi := CompareText(ExtractFileName(ExePath), 'msiexec.exe') = 0;
  if IsMsi then begin
    P := Pos('{', Args);
    if P = 0 then
      Exit;
    ProductCode := Copy(Args, P, 38);
    if (Length(ProductCode) <> 38) or (ProductCode[38] <> '}') then
      Exit;
    ExePath := ExpandConstant('{sys}\msiexec.exe');
    Args := '/x ' + ProductCode + ' /qn /norestart';
  end else begin
    // Tauri NSIS；优先采用登记的静默命令，避免丢失必要参数。
    if RegQueryStringValue(Entry.RootKey, UninstallKey + '\' + Entry.SubKey,
         'QuietUninstallString', Command) and (Trim(Command) <> '') then begin
      if not SplitExeCommand(Command, ExePath, Args) then
        Exit;
    end else
      Args := Trim('/S ' + Args);
    if (CompareText(ExtractFileName(ExePath), 'uninstall.exe') <> 0) or
       not FileExists(ExePath) then
      Exit;
  end;
  Log('卸载旧版本：' + ExePath + ' ' + Args);
  if not Exec(ExePath, Args, '', SW_HIDE, ewWaitUntilTerminated, ResultCode) then begin
    Log('无法启动旧版卸载器：' + IntToStr(ResultCode));
    Exit;
  end;
  Log('旧版卸载器退出码：' + IntToStr(ResultCode));
  if (ResultCode <> 0) and not (IsMsi and (ResultCode = 3010)) then
    Exit;
  if IsMsi and (ResultCode = 3010) then
    g_RestartRequired := True;
  // NSIS 可能通过临时副本继续卸载，不能仅凭启动成功报告完成。
  for Attempt := 1 to 100 do begin
    if not RegKeyExists(Entry.RootKey, UninstallKey + '\' + Entry.SubKey) then begin
      Result := True;
      Exit;
    end;
    Sleep(100);
  end;
  Log('旧版卸载条目仍存在：' + Entry.SubKey);
end;

function NeedRestart: Boolean;
begin
  Result := g_RestartRequired;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  I: Integer;
begin
  Result := '';
  CaptureRunEntry;
  if WizardIsTaskSelected('uninstallold') then
    for I := 0 to GetArrayLength(g_OldVersions) - 1 do
      if not TryUninstallEntry(g_OldVersions[I]) then begin
        Result := '旧版本卸载未完成。请先从 Windows 设置中卸载旧版本，再重试安装。历史记录和设置将保留。';
        Exit;
      end;
end;

// 同目录升级无需改写；迁移路径时保留用户、提权级别、参数、触发器与 ACL。
procedure SyncStartupTasks(Remove: Boolean);
var
  Service, Folder, Tasks, Task, Definition, Action: Variant;
  I: Integer;
  ExePath, TaskName, MigrationError: String;
  ResultCode: Integer;
  MigrationLog: TArrayOfString;
begin
  Service := CreateOleObject('Schedule.Service');
  Service.Connect();
  Folder := Service.GetFolder('\');
  Tasks := Folder.GetTasks(1);
  for I := Tasks.Count downto 1 do begin
    Task := Tasks.Item(I);
    TaskName := Task.Name;
    if Pos('FloatPaste for ', TaskName) = 1 then begin
      Definition := Task.Definition;
      if Definition.Actions.Count = 1 then begin
        Action := Definition.Actions.Item(1);
        // 非 Exec 动作没有 Path 属性，跳过同名前缀的非应用任务。
        try
          ExePath := Action.Path;
        except
          ExePath := '';
        end;
        if ExePath <> '' then begin
          ExePath := RemoveQuotes(ExePath);
          if Remove then begin
            if CompareText(ExePath, ExpandConstant('{app}\{#AppExeName}')) = 0 then begin
              Log('删除自启任务：' + TaskName);
              if not Exec(ExpandConstant('{sys}\schtasks.exe'),
                   '/Delete /TN "' + TaskName + '" /F', '',
                   SW_HIDE, ewWaitUntilTerminated, ResultCode) or (ResultCode <> 0) then
                RaiseException('删除自启任务失败：' + TaskName);
            end;
          end else if IsUpgradeExecutable(ExePath) and
                      (CompareText(ExePath, ExpandConstant('{app}\{#AppExeName}')) <> 0) then begin
            // Inno 6.7.1 的 Variant 封送调用 RegisterTaskDefinition 会触发访问冲突，
            // 借道 PowerShell 执行同一 COM 序列：迁移路径，保留参数、主体与安全描述符。
            // 任务名与路径经环境变量传入，脚本保持纯 ASCII，避免编码与引号问题。
            SetTaskEnv('FLOATPASTE_TASK_NAME', TaskName);
            SetTaskEnv('FLOATPASTE_TASK_PATH', ExpandConstant('{app}\{#AppExeName}'));
            SetTaskEnv('FLOATPASTE_TASK_ERRFILE', ExpandConstant('{tmp}\fp-task-migrate.err'));
            if not SaveStringToFile(ExpandConstant('{tmp}\fp-task-migrate.ps1'),
                 '$ErrorActionPreference = ''Stop''' + #13#10 +
                 'try {' + #13#10 +
                 '  $svc = New-Object -ComObject Schedule.Service' + #13#10 +
                 '  $svc.Connect()' + #13#10 +
                 '  $folder = $svc.GetFolder(''\'')' + #13#10 +
                 '  $name = $env:FLOATPASTE_TASK_NAME' + #13#10 +
                 '  $task = $folder.GetTask($name)' + #13#10 +
                 '  $def = $task.Definition' + #13#10 +
                 '  $act = $def.Actions.Item(1)' + #13#10 +
                 '  $act.Path = $env:FLOATPASTE_TASK_PATH' + #13#10 +
                 '  $folder.RegisterTaskDefinition($name, $def, 4, $def.Principal.UserId, $null, $def.Principal.LogonType, $task.GetSecurityDescriptor(4)) | Out-Null' + #13#10 +
                 '  exit 0' + #13#10 +
                 '} catch {' + #13#10 +
                 '  $_ | Out-String | Set-Content -Path $env:FLOATPASTE_TASK_ERRFILE' + #13#10 +
                 '  exit 1' + #13#10 +
                 '}', False) then
              RaiseException('无法写入任务迁移临时脚本。');
            MigrationError := '';
            if LoadStringsFromFile(ExpandConstant('{tmp}\fp-task-migrate.err'), MigrationLog) then
              for ResultCode := 0 to GetArrayLength(MigrationLog) - 1 do
                MigrationError := MigrationError + MigrationLog[ResultCode] + #13#10;
            DeleteFile(ExpandConstant('{tmp}\fp-task-migrate.err'));
            if not Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
                 '-NoProfile -ExecutionPolicy Bypass -File "' +
                 ExpandConstant('{tmp}\fp-task-migrate.ps1') + '"', '',
                 SW_HIDE, ewWaitUntilTerminated, ResultCode) or (ResultCode <> 0) then begin
              Log('自启任务迁移脚本失败，退出码=' + IntToStr(ResultCode) + '，错误=' + MigrationError);
              RaiseException('程序已安装，但迁移自启任务失败，请在应用设置中重新开启开机自启。');
            end;
            DeleteFile(ExpandConstant('{tmp}\fp-task-migrate.ps1'));
          end;
        end;
      end;
    end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  CurrentValue, NewCommand: String;
begin
  if CurStep <> ssPostInstall then
    Exit;
  if g_RunMigrate then
    // 旧卸载器可能删除 Run 项；若值被改成其他命令则不覆盖。
    if not RegQueryStringValue(HKEY_CURRENT_USER, RunKey, RunValueName, CurrentValue) or
       (CurrentValue = g_RunValue) then begin
      NewCommand := AddQuotes(ExpandConstant('{app}\{#AppExeName}'));
      if g_RunArgs <> '' then
        NewCommand := NewCommand + ' ' + g_RunArgs;
      if not RegWriteStringValue(HKEY_CURRENT_USER, RunKey, RunValueName, NewCommand) then
        RaiseException('程序已安装，但迁移开机自启失败，请在应用设置中重新开启。');
    end;
  try
    SyncStartupTasks(False);
  except
    Log('自启任务迁移失败：' + GetExceptionMessage);
    SuppressibleMsgBox('程序已安装，但迁移自启任务失败，请在应用设置中重新开启开机自启。',
      mbError, MB_OK, IDOK);
  end;
end;

// 卸载确认后才关闭实例，按完整路径定位，避免结束便携版或其他安装。
procedure CloseInstalledApp;
var
  Locator, Services, Processes, Process: Variant;
  I, Attempt: Integer;
  ExePath, ProcessId: String;
  Stopped: Boolean;
begin
  Locator := CreateOleObject('WbemScripting.SWbemLocator');
  Services := Locator.ConnectServer('.', 'root\CIMV2');
  Processes := Services.ExecQuery('SELECT * FROM Win32_Process WHERE Name = ''{#AppExeName}''');
  for I := 0 to Processes.Count - 1 do begin
    Process := Processes.ItemIndex(I);
    if not VarIsNull(Process.ExecutablePath) then begin
      ExePath := Process.ExecutablePath;
      if CompareText(ExePath, ExpandConstant('{app}\{#AppExeName}')) = 0 then begin
        ProcessId := Process.ProcessId;
        if Process.Terminate(0) <> 0 then
          RaiseException('无法关闭 FloatPaste，请从托盘退出后重试卸载。');
        Stopped := False;
        for Attempt := 1 to 50 do begin
          if Services.ExecQuery('SELECT ProcessId FROM Win32_Process WHERE ProcessId = ' + ProcessId).Count = 0 then begin
            Stopped := True;
            Break;
          end;
          Sleep(100);
        end;
        if not Stopped then
          RaiseException('等待 FloatPaste 退出超时，请从托盘退出后重试卸载。');
      end;
    end;
  end;
end;

procedure RestoreWinV;
var
  Value, Kept: String;
  I: Integer;
  Success: Boolean;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, ExplorerAdvancedKey, DisabledHotkeysValue, Value) then
    Exit;
  Kept := '';
  for I := 1 to Length(Value) do
    if (Value[I] <> 'V') and (Value[I] <> 'v') then
      Kept := Kept + Value[I];
  if Kept = Value then
    Exit;
  if Kept = '' then
    Success := RegDeleteValue(HKEY_CURRENT_USER, ExplorerAdvancedKey, DisabledHotkeysValue)
  else
    Success := RegWriteStringValue(HKEY_CURRENT_USER, ExplorerAdvancedKey, DisabledHotkeysValue, Kept);
  if not Success then
    RaiseException('清除 Win+V 接管失败，请检查当前用户的注册表权限。');
  if not UninstallSilent() then
    MsgBox('已清除 Win+V 接管配置。' + #13#10 +
      '若系统快捷键未恢复，请重启资源管理器或注销后重新登录。', mbInformation, MB_OK);
end;

// 这里只收集选项；关闭或取消窗口不会结束应用、修改系统配置或删除数据。
// 直接复用原生卸载向导的标题区、图标、白色页面和底部按钮栏。
function ShowUninstallDataOptions(var RemoveData: Boolean; DataDir: String): Boolean;
var
  Form: TUninstallProgressForm;
  Intro, Details: TNewStaticText;
  DeleteData: TNewCheckBox;
  ContinueButton: TNewButton;
  PageName, PageDescription: String;
begin
  Form := UninstallProgressForm;
  PageName := Form.PageNameLabel.Caption;
  PageDescription := Form.PageDescriptionLabel.Caption;
  Intro := TNewStaticText.Create(Form);
  Details := TNewStaticText.Create(Form);
  DeleteData := TNewCheckBox.Create(Form);
  ContinueButton := TNewButton.Create(Form);
  try
    Form.PageNameLabel.Caption := '选择卸载选项';
    Form.PageDescriptionLabel.Caption := '请选择是否同时清理应用数据。';
    Form.StatusLabel.Visible := False;
    Form.ProgressBar.Visible := False;

    Intro.Parent := Form.InstallingPage;
    Intro.AutoSize := False;
    Intro.ShowAccelChar := False;
    Intro.SetBounds(Form.StatusLabel.Left, Form.StatusLabel.Top,
      Form.StatusLabel.Width, ScaleY(40));
    Intro.WordWrap := True;
    Intro.Caption := '默认保留历史记录与设置，方便以后重新安装。如需清理，请勾选以下选项，然后单击“卸载”。';

    DeleteData.Parent := Form.InstallingPage;
    DeleteData.SetBounds(Intro.Left, Intro.Top + Intro.Height + ScaleY(12),
      Intro.Width, ScaleY(24));
    DeleteData.Caption := '同时删除历史记录、图片、设置和日志(&D)';
    DeleteData.Checked := RemoveData;

    Details.Parent := Form.InstallingPage;
    Details.AutoSize := False;
    Details.ShowAccelChar := False;
    Details.SetBounds(Intro.Left, DeleteData.Top + DeleteData.Height + ScaleY(12),
      Intro.Width, ScaleY(120));
    Details.WordWrap := True;
    Details.Caption := '删除后无法恢复，也会清理旧版遗留数据。安装版与便携版共用数据，请先退出其他 FloatPaste 实例。' + #13#10#13#10 +
      '数据目录：' + DataDir;

    ContinueButton.Parent := Form;
    ContinueButton.SetBounds(Form.CancelButton.Left - Form.CancelButton.Width - ScaleX(8),
      Form.CancelButton.Top, Form.CancelButton.Width, Form.CancelButton.Height);
    ContinueButton.Caption := '卸载(&U)';
    ContinueButton.ModalResult := mrOK;
    ContinueButton.Default := True;
    Form.CancelButton.Enabled := True;
    Form.CancelButton.ModalResult := mrCancel;
    Form.ActiveControl := ContinueButton;
    Result := Form.ShowModal() = mrOK;
    if Result then
      RemoveData := DeleteData.Checked;
  finally
    ContinueButton.Free();
    DeleteData.Free();
    Details.Free();
    Intro.Free();
    Form.CancelButton.ModalResult := 0;
    Form.CancelButton.Enabled := False;
    Form.PageNameLabel.Caption := PageName;
    Form.PageDescriptionLabel.Caption := PageDescription;
    Form.StatusLabel.Visible := True;
    Form.ProgressBar.Visible := True;
  end;
end;

function InitializeUninstall: Boolean;
var
  I: Integer;
begin
  g_UninstallDataDir := ExpandFileName(ExpandConstant('{userappdata}\floatpaste'));
  // 静默卸载只接受显式开关，/SUPPRESSMSGBOXES 不代表同意删除数据。
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/PURGEDATA') = 0 then
      g_RemoveUserData := True;
  Result := True;
end;

procedure InitializeUninstallProgressForm;
begin
  // 原生确认已通过；此时尚未结束实例或删除文件，取消仍可安全退出。
  if not UninstallSilent() then
    if not ShowUninstallDataOptions(g_RemoveUserData, g_UninstallDataDir) then
      Abort;
end;

procedure RemoveUninstallData;
var
  UserAppData, FolderName, DataDir, FailedDirs: String;
  I: Integer;
begin
  if not g_RemoveUserData then begin
    Log('保留用户数据：' + g_UninstallDataDir);
    Exit;
  end;
  // 仅清理两个固定目录，避免遗留数据在下次启动时重新迁回。
  UserAppData := RemoveBackslash(ExpandFileName(ExpandConstant('{userappdata}')));
  FailedDirs := '';
  for I := 0 to 1 do begin
    if I = 0 then
      FolderName := 'floatpaste'
    else
      FolderName := 'com.floatpaste';
    DataDir := ExpandFileName(AddBackslash(UserAppData) + FolderName);
    if (CompareText(ExtractFileDir(DataDir), UserAppData) <> 0) or
       (CompareText(ExtractFileName(DataDir), FolderName) <> 0) then
      RaiseException('用户数据目录校验失败，已跳过数据清理。');
    Log('删除用户数据：' + DataDir);
    // DelTree 只移除重解析点自身，不递归进入它指向的其他目录。
    if DirExists(DataDir) then
      if not DelTree(DataDir, True, True, True) then
        FailedDirs := FailedDirs + #13#10 + DataDir;
  end;
  if FailedDirs <> '' then
    RaiseException('部分用户数据未能删除。请退出其他 FloatPaste 实例后检查：' + FailedDirs);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Value, ExePath, Args: String;
begin
  if CurUninstallStep = usUninstall then begin
    // 先清理自启再结束进程，避免任务在删除程序期间再次拉起实例。
    SyncStartupTasks(True);
    CloseInstalledApp;
    if RegQueryStringValue(HKEY_CURRENT_USER, RunKey, RunValueName, Value) then
      if SplitExeCommand(Value, ExePath, Args) then
        if CompareText(ExePath, ExpandConstant('{app}\{#AppExeName}')) = 0 then
          if not RegDeleteValue(HKEY_CURRENT_USER, RunKey, RunValueName) then
            RaiseException('清除开机自启失败，请检查当前用户的注册表权限。');
  end else if CurUninstallStep = usPostUninstall then begin
    try
      RestoreWinV;
    finally
      RemoveUninstallData;
    end;
  end;
end;
