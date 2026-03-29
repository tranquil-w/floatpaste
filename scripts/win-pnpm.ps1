[CmdletBinding()]
param(
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$Args
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $repoRoot

$rtkCommand = Get-Command "rtk.exe" -ErrorAction SilentlyContinue

$pnpmCommand = Get-Command "pnpm.cmd" -ErrorAction SilentlyContinue
if (-not $pnpmCommand) {
  $pnpmCommand = Get-Command "pnpm" -ErrorAction SilentlyContinue
}

if (-not $pnpmCommand) {
  $defaultPnpmPath = Join-Path $env:APPDATA "npm\pnpm.cmd"
  if (Test-Path $defaultPnpmPath) {
    $pnpmPath = $defaultPnpmPath
  }
  else {
    throw "未找到 Windows pnpm，请先在 Windows 环境安装 Node.js 与 pnpm。"
  }
}
else {
  $pnpmPath = $pnpmCommand.Source
}

$global:LASTEXITCODE = 0
if ($rtkCommand) {
  & $rtkCommand.Source $pnpmPath @Args
}
else {
  & $pnpmPath @Args
}
exit $global:LASTEXITCODE
