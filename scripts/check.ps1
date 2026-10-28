#Requires -Version 7.2
[CmdletBinding()]
param(
    [ValidateSet('All', 'Rust', 'Scripts')]
    [string]$Scope = 'All',
    [switch]$UiSnapshots
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$repoRoot = Split-Path $PSScriptRoot -Parent
$runId = (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
$runDir = Join-Path $repoRoot ".artifacts/checks/$runId"
$reportPath = Join-Path $runDir 'result.json'
$steps = [System.Collections.Generic.List[object]]::new()

function Add-Check([string]$Name, [string]$Program, [string[]]$Arguments) {
    $steps.Add([ordered]@{
        name = $Name; program = $Program; arguments = $Arguments
        status = 'not_run'; exit_code = $null; log = "$Name.log"
    })
}

function Get-SourceSnapshot {
    $head = & git rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw '无法读取 Git 提交。' }
    $status = @(& git -c core.quotepath=false status --short --untracked-files=all)
    if ($LASTEXITCODE -ne 0) { throw '无法读取工作区状态。' }
    $files = & git ls-files --cached --others --exclude-standard -z
    if ($LASTEXITCODE -ne 0) { throw '无法枚举验证源码。' }
    # 哈希包含未跟踪源码与删除标记，排除 Git 忽略的构建产物和报告。
    $entries = foreach ($file in (($files -join "`n").Split([char]0, [StringSplitOptions]::RemoveEmptyEntries) | Sort-Object -Unique)) {
        $path = Join-Path $repoRoot $file
        $hash = if (Test-Path -LiteralPath $path -PathType Leaf) { (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash } else { 'deleted' }
        "$file`0$hash"
    }
    $bytes = [Text.Encoding]::UTF8.GetBytes(($entries -join "`n"))
    $digest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes))
    return [ordered]@{ head = "$head"; status = $status; content_sha256 = $digest }
}

if ($Scope -in 'All', 'Scripts') {
    Add-Check 'scripts' 'node' @('--test', 'scripts/check.test.mjs', 'scripts/release-notes.test.mjs')
}
if ($Scope -in 'All', 'Rust') {
    Add-Check 'fmt' 'cargo' @('fmt', '--all', '--', '--check')
    Add-Check 'rust-tests' 'cargo' @('test', '--locked', '-p', 'floatpaste-core', '-p', 'floatpaste-native', '--', '--nocapture')
    Add-Check 'native-build' 'cargo' @('build', '--locked', '-p', 'floatpaste-native')
}
if ($UiSnapshots) {
    foreach ($scale in '1', '1.25', '1.5') {
        Add-Check "ui-$scale" 'cargo' @('test', '--locked', '-p', 'floatpaste-native', '--bin', 'floatpaste', 'l2_headless_ui_contracts', '--', '--nocapture')
    }
}

$report = [ordered]@{
    scope = $Scope; ui_snapshots = [bool]$UiSnapshots; status = 'running'
    environment = [ordered]@{ os = [Environment]::OSVersion.VersionString; powershell = "$($PSVersionTable.PSVersion)"; repo = $repoRoot }
    started_at = [DateTimeOffset]::Now.ToString('o'); finished_at = $null
    source_start = $null; source_end = $null; source_changed = $null
    steps = $steps; coverage_notes = @(); error = $null
}
$exitCode = 1
$savedEnv = @{}
$savedConsoleEncoding = [Console]::OutputEncoding
$savedOutputEncoding = $OutputEncoding
Push-Location $repoRoot
try {
    New-Item -ItemType Directory -Path $runDir -ErrorAction Stop | Out-Null
    # Node 与 Rust 输出 UTF-8，不能按宿主 Windows 代码页解码中文证据。
    [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
    $OutputEncoding = [Console]::OutputEncoding
    $report.source_start = Get-SourceSnapshot
    foreach ($name in 'FLOATPASTE_E2E_STRICT', 'FLOATPASTE_E2E_KEEP', 'FLOATPASTE_E2E_EXE', 'FLOATPASTE_UI_SNAPSHOTS', 'SLINT_SCALE_FACTOR') {
        $savedEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
    }
    $env:FLOATPASTE_E2E_STRICT = '1'
    $env:FLOATPASTE_E2E_KEEP = '1'
    # 完整门槛以默认呈现配置运行；截图步骤再逐档设置并经 Cargo 重编译。
    Remove-Item Env:FLOATPASTE_E2E_EXE, Env:FLOATPASTE_UI_SNAPSHOTS, Env:SLINT_SCALE_FACTOR -ErrorAction SilentlyContinue
    foreach ($step in $steps) {
        if ($step.name.StartsWith('ui-')) {
            $env:FLOATPASTE_UI_SNAPSHOTS = '1'
            $env:SLINT_SCALE_FACTOR = $step.name.Substring(3)
        }
        $step.status = 'running'
        $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $reportPath -Encoding utf8
        $logPath = Join-Path $runDir $step.log
        New-Item -ItemType File -Path $logPath -ErrorAction Stop | Out-Null
        Write-Host "[$($step.name)] $($step.program) $($step.arguments -join ' ')"
        $program = $step.program
        $commandArgs = $step.arguments
        & $program @commandArgs 2>&1 | Tee-Object -FilePath $logPath | ForEach-Object { Write-Host $_ }
        $step.exit_code = $LASTEXITCODE
        $step.status = if ($step.exit_code -eq 0) { 'passed' } else { 'failed' }
        $report.coverage_notes += @(Get-Content -LiteralPath $logPath | Where-Object {
            $_ -match '::warning::|跳过.*(焦点|前台)|(焦点|前台).*未覆盖|^test .* \.\.\. ignored'
        } | ForEach-Object { "[$($step.name)] $_" })
        if ($step.exit_code -ne 0) { throw "检查 $($step.name) 失败，退出码 $($step.exit_code)。" }
    }
    $report.status = 'passed'
    $exitCode = 0
}
catch {
    $report.status = 'failed'
    $report.error = $_.Exception.Message
    foreach ($step in $steps) {
        if ($step.status -eq 'running') { $step.status = 'failed' }
    }
    Write-Host $report.error -ForegroundColor Red
}
finally {
    foreach ($name in $savedEnv.Keys) {
        [Environment]::SetEnvironmentVariable($name, $savedEnv[$name], 'Process')
    }
    try {
        $report.source_end = Get-SourceSnapshot
        $report.source_changed = ($report.source_start | ConvertTo-Json -Compress) -cne ($report.source_end | ConvertTo-Json -Compress)
        if ($report.source_changed) {
            $report.status = 'failed'
            $report.error = (@($report.error, '执行期间源码状态发生变化，需按影响重新验证。') | Where-Object { $_ }) -join ' '
            $exitCode = 1
        }
        $report.finished_at = [DateTimeOffset]::Now.ToString('o')
        $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $reportPath -Encoding utf8
        Write-Host "检查结果：$($report.status)；报告：$reportPath"
    }
    catch {
        $exitCode = 1
        Write-Host "无法保存验证报告：$($_.Exception.Message)" -ForegroundColor Red
    }
    [Console]::OutputEncoding = $savedConsoleEncoding
    $OutputEncoding = $savedOutputEncoding
    Pop-Location
}
exit $exitCode
