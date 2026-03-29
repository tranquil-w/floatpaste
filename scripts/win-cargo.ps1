[CmdletBinding()]
param(
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$Args
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$cargoRoot = Join-Path $repoRoot "src-tauri"
Set-Location $cargoRoot

$rtkCommand = Get-Command "rtk.exe" -ErrorAction SilentlyContinue

$cargoCommand = Get-Command "cargo.exe" -ErrorAction SilentlyContinue
if (-not $cargoCommand) {
  $defaultCargoPath = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
  if (Test-Path $defaultCargoPath) {
    $cargoPath = $defaultCargoPath
  }
  else {
    throw "未找到 Windows cargo.exe，请先在 Windows 环境安装 Rust 工具链。"
  }
}
else {
  $cargoPath = $cargoCommand.Source
}

$global:LASTEXITCODE = 0
if ($rtkCommand) {
  & $rtkCommand.Source $cargoPath @Args
}
else {
  & $cargoPath @Args
}
exit $global:LASTEXITCODE
