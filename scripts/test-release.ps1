# 在 GitHub 的临时 Windows runner 上验收交付产物，复用真进程 E2E。
param([Parameter(Mandatory)][string]$Version)

$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') {
    throw '安装与卸载验收只能在 GitHub Actions 临时 runner 上运行，避免修改本机安装。'
}

$assetDir = Join-Path $PWD 'tmp/assets'
$testRoot = Join-Path $env:RUNNER_TEMP "floatpaste-release-$([guid]::NewGuid())"
$portableDir = Join-Path $testRoot 'portable'
$installDir = Join-Path $testRoot 'installed'
$setup = Join-Path $assetDir "FloatPaste_${Version}_x64-setup.exe"
$installedExe = Join-Path $installDir 'floatpaste.exe'
$uninstaller = Join-Path $installDir 'unins000.exe'
$uninstallKey = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{6C4F0E9A-2B7D-4E3A-9F81-A05C8D1B2E47}_is1'

function Test-App([string]$Executable) {
    if (!(Test-Path -LiteralPath $Executable -PathType Leaf)) { throw "程序不存在：$Executable" }
    $env:FLOATPASTE_E2E_EXE = $Executable
    $env:FLOATPASTE_E2E_STRICT = '1'
    $env:FLOATPASTE_E2E_KEEP = '1'
    cargo test --locked -p floatpaste-native --test e2e_smoke -- --nocapture
    if ($LASTEXITCODE -ne 0) { throw "交付程序 E2E 失败：$Executable" }
}

Expand-Archive -LiteralPath (Join-Path $assetDir "FloatPaste-v${Version}-windows-x64-portable.zip") -DestinationPath $portableDir
if (!(Test-Path -LiteralPath (Join-Path $portableDir 'README-使用说明.md'))) {
    throw '便携包缺少使用说明。'
}
$portableExe = Join-Path $portableDir 'floatpaste.exe'
$expectedHash = (Get-FileHash -LiteralPath 'target/release/floatpaste.exe' -Algorithm SHA256).Hash
if ((Get-FileHash -LiteralPath $portableExe -Algorithm SHA256).Hash -ne $expectedHash) {
    throw '便携包中的程序与发布构建不一致。'
}
Test-App $portableExe

# /TASKS= 禁用旧版卸载和桌面快捷方式；不让安装器启动应用，由 E2E 隔离启动。
try {
    $setupArgs = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', '/TASKS=', "/DIR=`"$installDir`"", "/LOG=`"$testRoot/install.log`"")
    $process = Start-Process -FilePath $setup -ArgumentList $setupArgs -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode -ne 0) { throw "安装失败：$($process.ExitCode)" }
    if ((Get-ItemProperty -LiteralPath $uninstallKey).DisplayVersion -ne $Version) {
        throw '安装后的版本号与 tag 不一致。'
    }
    if ((Get-FileHash -LiteralPath $installedExe -Algorithm SHA256).Hash -ne $expectedHash) {
        throw '安装后的程序与发布构建不一致。'
    }
    Test-App $installedExe
} finally {
    if (Test-Path -LiteralPath $uninstaller) {
        $process = Start-Process -FilePath $uninstaller -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait -PassThru -WindowStyle Hidden
        if ($process.ExitCode -ne 0) { throw "卸载失败：$($process.ExitCode)" }
        # Inno 卸载器由临时副本收尾，等原目录中的程序和注册项清理完成。
        for ($attempt = 0; $attempt -lt 50; $attempt++) {
            if (!(Test-Path -LiteralPath $installedExe) -and !(Test-Path -LiteralPath $uninstallKey)) { break }
            Start-Sleep -Milliseconds 200
        }
        if ((Test-Path -LiteralPath $installedExe) -or (Test-Path -LiteralPath $uninstallKey)) {
            throw '卸载后仍存在程序或卸载注册项。'
        }
    } else {
        throw '安装包未生成卸载程序。'
    }
}
Write-Output '便携版和安装版 E2E、安装版本、程序哈希与卸载检查通过。'
