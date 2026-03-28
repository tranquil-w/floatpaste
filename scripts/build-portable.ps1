[CmdletBinding()]
param(
  [string]$Version,
  [string]$ReleaseNotesPath,
  [switch]$SkipChecks
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $repoRoot

function Write-Step {
  param([string]$Message)
  Write-Host ""
  Write-Host "==> $Message" -ForegroundColor Cyan
}

function Get-JsonVersion {
  param([string]$Path)
  return (Get-Content $Path -Raw -Encoding UTF8 | ConvertFrom-Json).version
}

function Get-CargoVersion {
  param([string]$Path)

  $content = Get-Content $Path -Raw -Encoding UTF8
  $match = [regex]::Match($content, '(?m)^version\s*=\s*"([^"]+)"')
  if (-not $match.Success) {
    throw "无法从 Cargo.toml 读取版本号：$Path"
  }

  return $match.Groups[1].Value
}

function Resolve-OptionalPath {
  param([string]$Path)

  if (-not $Path) {
    return $null
  }

  if ([System.IO.Path]::IsPathRooted($Path)) {
    return $Path
  }

  return (Join-Path $repoRoot $Path)
}

function Get-Sha256Hash {
  param([string]$Path)

  $stream = [System.IO.File]::OpenRead($Path)
  try {
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
      $hashBytes = $sha256.ComputeHash($stream)
      return ([System.BitConverter]::ToString($hashBytes)).Replace("-", "").ToLowerInvariant()
    }
    finally {
      $sha256.Dispose()
    }
  }
  finally {
    $stream.Dispose()
  }
}

function Read-Utf8Text {
  param([string]$Path)

  return Get-Content $Path -Raw -Encoding UTF8
}

function Write-Utf8BomText {
  param(
    [string]$Path,
    [string]$Content
  )

  $utf8Bom = New-Object System.Text.UTF8Encoding($true)
  [System.IO.File]::WriteAllText($Path, $Content, $utf8Bom)
}

if (-not $Version) {
  $Version = Get-JsonVersion -Path (Join-Path $repoRoot "package.json")
}

$buildProfile = "release"
$exeName = "floatpaste.exe"
$artifactName = "FloatPaste-v${Version}-windows-x64-portable"
$releaseRoot = Join-Path $repoRoot "tmp\release"
$stagingDir = Join-Path $releaseRoot $artifactName
$zipPath = Join-Path $releaseRoot "${artifactName}.zip"
$checksumPath = Join-Path $releaseRoot "SHA256SUMS.txt"
$exeSource = Join-Path $repoRoot "src-tauri\target\${buildProfile}\${exeName}"
$defaultReleaseNotesPath = Join-Path $repoRoot "docs\release\发布说明模板.md"
$defaultTesterGuidePath = Join-Path $repoRoot "docs\release\使用说明模板.md"
$resolvedReleaseNotesPath = Resolve-OptionalPath -Path $ReleaseNotesPath

if (-not $resolvedReleaseNotesPath) {
  $resolvedReleaseNotesPath = $defaultReleaseNotesPath
}

if (-not (Test-Path $resolvedReleaseNotesPath)) {
  throw "未找到发布说明模板：$resolvedReleaseNotesPath"
}

if (-not (Test-Path $defaultTesterGuidePath)) {
  throw "未找到测试版使用说明模板：$defaultTesterGuidePath"
}

Write-Step "校验版本一致性"
$packageVersion = Get-JsonVersion -Path (Join-Path $repoRoot "package.json")
$tauriVersion = Get-JsonVersion -Path (Join-Path $repoRoot "src-tauri\tauri.conf.json")
$cargoVersion = Get-CargoVersion -Path (Join-Path $repoRoot "src-tauri\Cargo.toml")

if (($packageVersion -ne $tauriVersion) -or ($packageVersion -ne $cargoVersion)) {
  throw "版本不一致：package.json=${packageVersion}, tauri.conf.json=${tauriVersion}, Cargo.toml=${cargoVersion}"
}

if ($Version -ne $packageVersion) {
  throw "传入版本 ${Version} 与 package.json 版本 ${packageVersion} 不一致，请先更新仓库版本号。"
}

if (-not $SkipChecks) {
  Write-Step "执行前端构建检查"
  & pnpm build
  if ($LASTEXITCODE -ne 0) {
    throw "pnpm build 执行失败。"
  }

  Write-Step "执行 Rust 编译检查"
  & cargo check --manifest-path "src-tauri/Cargo.toml"
  if ($LASTEXITCODE -ne 0) {
    throw "cargo check 执行失败。"
  }
}

Write-Step "生成 Tauri release 可执行文件"
& pnpm tauri build --no-bundle --ci
if ($LASTEXITCODE -ne 0) {
  throw "pnpm tauri build --no-bundle --ci 执行失败。"
}

if (-not (Test-Path $exeSource)) {
  throw "未找到预期产物：${exeSource}"
}

Write-Step "准备便携包目录"
New-Item -ItemType Directory -Path $releaseRoot -Force | Out-Null

if (Test-Path $stagingDir) {
  Remove-Item $stagingDir -Recurse -Force
}

if (Test-Path $zipPath) {
  Remove-Item $zipPath -Force
}

New-Item -ItemType Directory -Path $stagingDir | Out-Null

$releaseNotesContent = Read-Utf8Text -Path $resolvedReleaseNotesPath
$testerGuideContent = Read-Utf8Text -Path $defaultTesterGuidePath
$buildDate = Get-Date -Format "yyyy-MM-dd"

$releaseNotesContent = $releaseNotesContent.Replace("{{version}}", $Version).Replace("{{date}}", $buildDate)
$testerGuideContent = $testerGuideContent.Replace("{{version}}", $Version).Replace("{{date}}", $buildDate)

Copy-Item $exeSource (Join-Path $stagingDir $exeName)
Write-Utf8BomText -Path (Join-Path $stagingDir "CHANGELOG-${Version}.md") -Content $releaseNotesContent
Write-Utf8BomText -Path (Join-Path $stagingDir "README-测试版使用说明.md") -Content $testerGuideContent

Write-Step "压缩便携包"
Compress-Archive -Path (Join-Path $stagingDir "*") -DestinationPath $zipPath -Force

Write-Step "生成校验文件"
$zipHash = Get-Sha256Hash -Path $zipPath
$exeHash = Get-Sha256Hash -Path (Join-Path $stagingDir $exeName)
$checksumLines = @(
  "$zipHash *$([System.IO.Path]::GetFileName($zipPath))"
  "$exeHash *${artifactName}/${exeName}"
)
Set-Content -Path $checksumPath -Value $checksumLines -Encoding ASCII

Write-Step "生成完成"
Write-Host "便携包: ${zipPath}"
Write-Host "校验文件: ${checksumPath}"
Write-Host "解包目录: ${stagingDir}"
