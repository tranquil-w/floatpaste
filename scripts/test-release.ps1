# 在 GitHub 的临时 Windows runner 上验收交付产物，复用真进程 E2E。
param([Parameter(Mandatory)][string]$Version)

$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') {
    throw '安装与卸载验收只能在 GitHub Actions 临时 runner 上运行，避免修改本机安装。'
}

$assetDir = Join-Path $PWD 'tmp/assets'
$testRoot = Join-Path $env:RUNNER_TEMP "floatpaste-release-$([guid]::NewGuid())"
$portableDir = Join-Path $testRoot 'portable'
$installDir = Join-Path $testRoot 'installed with spaces'
$legacyDir = Join-Path $testRoot 'legacy with spaces'
$foreignDir = Join-Path $testRoot 'another copy'
$setup = Join-Path $assetDir "FloatPaste_$($Version)_x64-setup.exe"
$installedExe = Join-Path $installDir 'floatpaste.exe'
$legacyExe = Join-Path $legacyDir 'uninstall.exe'
$foreignExe = Join-Path $foreignDir 'floatpaste.exe'
$uninstaller = Join-Path $installDir 'unins000.exe'
$uninstallKey = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{6C4F0E9A-2B7D-4E3A-9F81-A05C8D1B2E47}_is1'
$legacyKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\FloatPaste-Installer-Legacy-Test'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$advancedKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced'
$taskName = "FloatPaste for installer-test-$([guid]::NewGuid())"
$foreignTaskName = "$taskName-portable"
$userDataDir = Join-Path $env:APPDATA 'floatpaste'
$legacyUserDataDir = Join-Path $env:APPDATA 'com.floatpaste'
$dataMarker = Join-Path $userDataDir "installer-retention-$([guid]::NewGuid()).txt"
$foreignProcess = $null
$installedProcess = $null

function Test-App([string]$Executable) {
    if (!(Test-Path -LiteralPath $Executable -PathType Leaf)) { throw "程序不存在：$Executable" }
    $env:FLOATPASTE_E2E_EXE = $Executable
    $env:FLOATPASTE_E2E_STRICT = '1'
    $env:FLOATPASTE_E2E_KEEP = '1'
    cargo test --locked -p floatpaste-native --test e2e_smoke -- --nocapture
    if ($LASTEXITCODE -ne 0) { throw "交付程序 E2E 失败：$Executable" }
}

function Invoke-Setup([string]$LogName, [string]$Tasks = 'uninstallold') {
    $setupArgs = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', "/TASKS=$Tasks",
        ('/DIR="{0}"' -f $installDir), ('/LOG="{0}"' -f (Join-Path $testRoot "$LogName.log")))
    $process = Start-Process -FilePath $setup -ArgumentList $setupArgs -Wait -PassThru -WindowStyle Hidden
    return $process.ExitCode
}

function Uninstall-App([switch]$PurgeData) {
    if (!(Test-Path -LiteralPath $uninstaller)) { throw '安装包未生成卸载程序。' }
    $uninstallArgs = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART',
        ('/LOG="{0}"' -f (Join-Path $testRoot 'uninstall.log')))
    if ($PurgeData) { $uninstallArgs += '/PURGEDATA' }
    $process = Start-Process -FilePath $uninstaller -ArgumentList $uninstallArgs -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode -ne 0) { throw "卸载失败：$($process.ExitCode)" }
    # Inno 卸载器由临时副本收尾，等原目录中的程序和注册项清理完成。
    for ($attempt = 0; $attempt -lt 50; $attempt++) {
        if (!(Test-Path -LiteralPath $installedExe) -and !(Test-Path -LiteralPath $uninstallKey)) { break }
        Start-Sleep -Milliseconds 200
    }
    if ((Test-Path -LiteralPath $installedExe) -or (Test-Path -LiteralPath $uninstallKey)) {
        throw '卸载后仍存在程序或卸载注册项。'
    }
}

function Assert-ForeignCopy {
    $foreignProcess.Refresh()
    if ($foreignProcess.HasExited) { throw '安装或卸载误结束了其他路径下的同名进程。' }
    $task = Get-ScheduledTask -TaskName $foreignTaskName
    if ($task.Actions.Execute -ne $foreignExe) { throw '其他路径的自启任务被修改。' }
}

function Start-InstalledApp {
    $start = [Diagnostics.ProcessStartInfo]::new($installedExe, '--silent')
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.EnvironmentVariables['APPDATA'] = Join-Path $testRoot 'running-app-data'
    $start.EnvironmentVariables['FLOATPASTE_INSTANCE_ID'] = "installer-$([guid]::NewGuid())"
    $start.EnvironmentVariables['FLOATPASTE_SKIP_AUTOSTART_SYNC'] = '1'
    $child = [Diagnostics.Process]::Start($start)
    if ($child.WaitForExit(1000)) { throw '安装版未保持运行，无法验收占用文件时的升级与卸载。' }
    return $child
}

Expand-Archive -LiteralPath (Join-Path $assetDir "FloatPaste-v$($Version)-windows-x64-portable.zip") -DestinationPath $portableDir
if (!(Test-Path -LiteralPath (Join-Path $portableDir 'README-使用说明.md'))) {
    throw '便携包缺少使用说明。'
}
$portableExe = Join-Path $portableDir 'floatpaste.exe'
$expectedHash = (Get-FileHash -LiteralPath 'target/release/floatpaste.exe' -Algorithm SHA256).Hash
if ((Get-FileHash -LiteralPath $portableExe -Algorithm SHA256).Hash -ne $expectedHash) {
    throw '便携包中的程序与发布构建不一致。'
}
Test-App $portableExe

if ((Test-Path -LiteralPath $uninstallKey) -or (Test-Path -LiteralPath $legacyKey)) {
    throw 'runner 已存在 FloatPaste 安装记录，停止验收。'
}
if ((Test-Path -LiteralPath $userDataDir) -or (Test-Path -LiteralPath $legacyUserDataDir)) {
    throw 'runner 已存在 FloatPaste 用户数据，停止数据清理验收。'
}
# 属性缺失时 Get-ItemPropertyValue 抛终止错误，-ErrorAction SilentlyContinue 无法抑制，须用 try/catch 取空
try { $originalRun = Get-ItemPropertyValue -LiteralPath $runKey -Name FloatPaste } catch { $originalRun = $null }
try { $originalHotkeys = Get-ItemPropertyValue -LiteralPath $advancedKey -Name DisabledHotkeys } catch { $originalHotkeys = $null }
New-Item -ItemType Directory -Path $legacyDir, $foreignDir, (Split-Path -Parent $dataMarker) -Force | Out-Null
Set-Content -LiteralPath $dataMarker -Value '保留历史记录和设置'
$csc = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
& $csc /nologo /target:exe /platform:x64 "/out:$legacyExe" (Join-Path $PWD 'packaging/tests/legacy-uninstaller.cs')
if ($LASTEXITCODE -ne 0) { throw '编译旧版卸载测试替身失败。' }
Copy-Item -LiteralPath $legacyExe -Destination $foreignExe
$principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive -RunLevel Highest

try {
    $legacyApp = Join-Path $legacyDir 'floatpaste.exe'
    foreach ($entry in @(@{ Name = $taskName; Exe = $legacyApp }, @{ Name = $foreignTaskName; Exe = $foreignExe })) {
        $action = New-ScheduledTaskAction -Execute $entry.Exe -Argument '--silent'
        Register-ScheduledTask -TaskName $entry.Name -Action $action -Principal $principal -Force | Out-Null
    }
    $foreignProcess = Start-Process -FilePath $foreignExe -ArgumentList '/wait' -PassThru -WindowStyle Hidden
    # 全新 runner profile 可能没有 HKCU Uninstall 父键，-Force 会补建中间键
    New-Item -Path $legacyKey -Force | Out-Null
    New-ItemProperty -LiteralPath $legacyKey -Name DisplayName -Value FloatPaste -Force | Out-Null
    # 未加引号且含空格的卸载路径，以及带必要参数的 QuietUninstallString。
    New-ItemProperty -LiteralPath $legacyKey -Name UninstallString -Value "$legacyExe /kept" -Force | Out-Null
    if (!(Test-Path -LiteralPath $runKey)) { New-Item -Path $runKey -Force | Out-Null }
    Set-ItemProperty -LiteralPath $runKey -Name FloatPaste -Value ('"{0}" --silent' -f $legacyApp)

    foreach ($failure in @('fail', 'keep')) {
        Set-ItemProperty -LiteralPath $legacyKey -Name QuietUninstallString -Value ('"{0}" /S /{1}' -f $legacyExe, $failure)
        if ((Invoke-Setup "legacy-$failure") -eq 0) { throw "旧版卸载异常 $failure 未阻止安装。" }
        if ((Test-Path -LiteralPath $installedExe) -or (Test-Path -LiteralPath $uninstallKey)) {
            throw '旧版卸载失败后仍写入了新安装。'
        }
        Assert-ForeignCopy
    }
    # 删除静默命令，成功路径直接覆盖未加引号的 UninstallString 解析。
    Remove-ItemProperty -LiteralPath $legacyKey -Name QuietUninstallString
    if ((Invoke-Setup 'install') -ne 0) { throw '旧版迁移安装失败。' }
    if (Test-Path -LiteralPath $legacyKey) { throw '旧版卸载注册项未清理。' }
    if ((Get-ItemPropertyValue -LiteralPath $runKey -Name FloatPaste) -ne ('"{0}" --silent' -f $installedExe)) {
        throw '旧 Run 项未迁移到安装位置或启动参数丢失。'
    }
    $migratedTask = Get-ScheduledTask -TaskName $taskName
    if (($migratedTask.Actions.Execute -ne $installedExe) -or
        ($migratedTask.Actions.Arguments -ne '--silent') -or ($migratedTask.Principal.RunLevel -ne 'Highest')) {
        throw '自启任务迁移未保留程序路径、启动参数或提权级别。'
    }
    $installedProcess = Start-InstalledApp
    if ((Invoke-Setup 'upgrade' '') -ne 0) { throw '程序运行时覆盖安装失败。' }
    if (!$installedProcess.WaitForExit(10000)) { throw '覆盖安装未关闭占用程序的实例。' }
    if ((Get-ItemProperty -LiteralPath $uninstallKey).DisplayVersion -ne $Version) {
        throw '安装后的版本号与 tag 不一致。'
    }
    if ((Get-FileHash -LiteralPath $installedExe -Algorithm SHA256).Hash -ne $expectedHash) {
        throw '安装后的程序与发布构建不一致。'
    }
    Assert-ForeignCopy
    Test-App $installedExe

    if (!(Test-Path -LiteralPath $advancedKey)) { New-Item -Path $advancedKey -Force | Out-Null }
    Set-ItemProperty -LiteralPath $advancedKey -Name DisabledHotkeys -Value 'AvSV'
    New-Item -ItemType Directory -Path $legacyUserDataDir | Out-Null
    $legacyDataMarker = Join-Path $legacyUserDataDir 'retained.txt'
    Set-Content -LiteralPath $legacyDataMarker -Value '旧版独有数据'
    $installedProcess = Start-InstalledApp
    Uninstall-App
    if (!$installedProcess.WaitForExit(10000)) { throw '卸载后当前安装实例仍在运行。' }
    if (Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue) { throw '卸载后自启任务残留。' }
    try { $residualRun = Get-ItemPropertyValue -LiteralPath $runKey -Name FloatPaste } catch { $residualRun = $null }
    if ($null -ne $residualRun) {
        throw '卸载后旧 Run 项残留。'
    }
    if ((Get-ItemPropertyValue -LiteralPath $advancedKey -Name DisabledHotkeys) -ne 'AS') {
        throw 'Win+V 未恢复或其他快捷键禁用项被破坏。'
    }
    if ((Get-Content -LiteralPath $dataMarker -Raw).Trim() -ne '保留历史记录和设置') {
        throw '卸载修改了用户数据。'
    }
    if ((Get-Content -LiteralPath $legacyDataMarker -Raw).Trim() -ne '旧版独有数据') {
        throw '默认卸载删除了旧版遗留数据。'
    }
    Assert-ForeignCopy

    # 再安装/卸载一次，验证不接管或删除便携版的 Run 项，也不改动无 V 的配置。
    $foreignRun = '"{0}" --silent' -f $foreignExe
    Set-ItemProperty -LiteralPath $runKey -Name FloatPaste -Value $foreignRun
    if ((Invoke-Setup 'reinstall' '') -ne 0) { throw '重新安装失败。' }
    # 覆盖嵌套数据与重解析点：清理共享数据目录，不能跟随链接删除外部文件。
    $protectedDir = Join-Path $testRoot 'protected-data'
    New-Item -ItemType Directory -Path $protectedDir, (Join-Path $userDataDir 'images') -Force | Out-Null
    $protectedFile = Join-Path $protectedDir 'keep.txt'
    Set-Content -LiteralPath $protectedFile -Value '不得删除'
    Set-Content -LiteralPath (Join-Path $userDataDir 'images/test.png') -Value '待清理图片'
    New-Item -ItemType Junction -Path (Join-Path $userDataDir 'linked-data') -Target $protectedDir | Out-Null
    Uninstall-App -PurgeData
    if (Test-Path -LiteralPath $userDataDir) { throw '显式清理后用户数据目录仍存在。' }
    if (Test-Path -LiteralPath $legacyUserDataDir) { throw '显式清理后旧版数据目录仍存在。' }
    if ((Get-Content -LiteralPath $protectedFile -Raw).Trim() -ne '不得删除') {
        throw '清理用户数据时误删除或修改了重解析点目标。'
    }
    if ((Get-ItemPropertyValue -LiteralPath $runKey -Name FloatPaste) -ne $foreignRun) {
        throw '其他路径的 Run 项被安装器改写或删除。'
    }
    if ((Get-ItemPropertyValue -LiteralPath $advancedKey -Name DisabledHotkeys) -ne 'AS') {
        throw '无 Win+V 接管时仍修改了快捷键配置。'
    }
    Assert-ForeignCopy
} finally {
    try {
        if (Test-Path -LiteralPath $uninstaller) { Uninstall-App }
    } finally {
        if ($installedProcess -and !$installedProcess.HasExited) { Stop-Process -Id $installedProcess.Id -Force }
        if ($foreignProcess -and !$foreignProcess.HasExited) { Stop-Process -Id $foreignProcess.Id -Force }
        foreach ($name in @($taskName, $foreignTaskName)) {
            if (Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue) {
                Unregister-ScheduledTask -TaskName $name -Confirm:$false
            }
        }
        # 测试卸载条目只有字符串值，没有子项，无需递归删除。
        if (Test-Path -LiteralPath $legacyKey) { Remove-Item -LiteralPath $legacyKey }
        foreach ($saved in @(
            @{ Key = $runKey; Name = 'FloatPaste'; Value = $originalRun },
            @{ Key = $advancedKey; Name = 'DisabledHotkeys'; Value = $originalHotkeys }
        )) {
            if ($null -ne $saved.Value) {
                Set-ItemProperty -LiteralPath $saved.Key -Name $saved.Name -Value $saved.Value
            } else {
                Remove-ItemProperty -LiteralPath $saved.Key -Name $saved.Name -ErrorAction SilentlyContinue
            }
        }
        Remove-Item -LiteralPath $dataMarker -ErrorAction SilentlyContinue
    }
}
Write-Output '便携版和安装版 E2E、旧版卸载失败处理、自启迁移、覆盖安装、默认保留/显式清理数据、重解析点与其他副本隔离检查通过。'
