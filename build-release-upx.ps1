param(
    [switch]$SkipBuild,
    [switch]$NoLzma,
    [string]$OutputRoot
)

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$VersionLine = Select-String -Path (Join-Path $Root "Cargo.toml") -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (!$VersionLine) {
    throw "无法从 Cargo.toml 读取版本号"
}
$Version = $VersionLine.Matches[0].Groups[1].Value
$PackageName = "SSL证书自动续期-v$Version-windows-x64"
$Upx = Join-Path $Root "upx-5.1.0-win64\upx.exe"
$ReleaseDir = Join-Path $Root "target\release"
$CliExe = Join-Path $ReleaseDir "ssl-renew-cli.exe"
$SignerExe = Join-Path $ReleaseDir "ssl-signer-agent.exe"
$GuiExe = Join-Path $ReleaseDir "SSL证书自动续期.exe"
$SignerBat = Join-Path $ReleaseDir "启动签发程序.bat"
$GuiDir = Join-Path $Root "ssl-renew-gui"
$PackageRoot = if ($OutputRoot) { $OutputRoot } else { Join-Path $ReleaseDir "package" }
$StageDir = Join-Path $PackageRoot $PackageName
$ZipPath = Join-Path $ReleaseDir "$PackageName.zip"

function Format-Size([long]$Bytes) {
    if ($Bytes -ge 1MB) {
        return "{0:N2} MB" -f ($Bytes / 1MB)
    }
    return "{0:N0} KB" -f ($Bytes / 1KB)
}

function Compress-Exe([string]$Path) {
    if (!(Test-Path $Path)) {
        throw "找不到 exe：$Path"
    }

    $Before = (Get-Item $Path).Length
    Write-Host ""
    Write-Host "UPX 压缩：$Path"
    Write-Host "压缩前：$(Format-Size $Before)"

    $Args = @("--best")
    if (!$NoLzma) {
        $Args += "--lzma"
    }
    $Args += $Path

    $Output = & $Upx @Args 2>&1
    $Code = $LASTEXITCODE
    $Output | ForEach-Object { Write-Host $_ }
    if ($Code -ne 0) {
        $Text = ($Output | Out-String)
        if ($Text -match "AlreadyPacked|AlreadyPackedException") {
            Write-Host "文件已被 UPX 压缩，跳过：$Path"
            return
        }
        throw "UPX 压缩失败：$Path"
    }

    $After = (Get-Item $Path).Length
    Write-Host "压缩后：$(Format-Size $After)"
}

function Write-Signer-Launcher([string]$Path) {
    $Content = @(
        "@echo off",
        "cd /d ""%~dp0""",
        "ssl-signer-agent.exe",
        "pause"
    ) -join "`r`n"
    Set-Content -Path $Path -Value $Content -Encoding ASCII
}

function Assert-ChildPath([string]$Parent, [string]$Child) {
    $ParentFull = [System.IO.Path]::GetFullPath($Parent).TrimEnd('\', '/')
    $ChildFull = [System.IO.Path]::GetFullPath($Child).TrimEnd('\', '/')
    if (!$ChildFull.StartsWith($ParentFull, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "路径不在预期目录内：$ChildFull"
    }
}

function Copy-ReleaseFile([string]$Source, [string]$DestinationName) {
    if (!(Test-Path $Source)) {
        throw "找不到发布文件：$Source"
    }
    Copy-Item -LiteralPath $Source -Destination (Join-Path $StageDir $DestinationName) -Force
}

if (!(Test-Path $Upx)) {
    throw "找不到 UPX：$Upx"
}

Push-Location $Root
try {
    if (!$SkipBuild) {
        Write-Host "构建 CLI..."
        cargo build --release -p ssl-renew-cli
        if ($LASTEXITCODE -ne 0) {
            throw "CLI 构建失败"
        }

        Write-Host ""
        Write-Host "构建 Signer..."
        cargo build --release -p ssl-signer-agent
        if ($LASTEXITCODE -ne 0) {
            throw "Signer 构建失败"
        }

        Write-Host ""
        Write-Host "构建 GUI 前端..."
        Push-Location $GuiDir
        try {
            npm run build
            if ($LASTEXITCODE -ne 0) {
                throw "GUI 前端构建失败"
            }
        } finally {
            Pop-Location
        }

        Write-Host ""
        Write-Host "构建 GUI exe..."
        cargo build --release -p ssl-renew-gui --bin "SSL证书自动续期"
        if ($LASTEXITCODE -ne 0) {
            throw "GUI exe 构建失败"
        }
    }

    Compress-Exe $CliExe
    Compress-Exe $SignerExe
    Compress-Exe $GuiExe
    Write-Signer-Launcher $SignerBat

    Assert-ChildPath $PackageRoot $StageDir
    if (Test-Path $StageDir) {
        Remove-Item -LiteralPath $StageDir -Recurse -Force
    }
    New-Item -ItemType Directory -Path $StageDir -Force | Out-Null

    Copy-ReleaseFile $GuiExe "SSL证书自动续期.exe"
    Copy-ReleaseFile $CliExe "ssl-renew-cli.exe"
    Copy-ReleaseFile $SignerExe "ssl-signer-agent.exe"
    Copy-ReleaseFile $SignerBat "启动签发程序.bat"
    Copy-ReleaseFile (Join-Path $Root "README.md") "README.md"
    Copy-ReleaseFile (Join-Path $Root "README.zh.md") "README.zh.md"
    Copy-ReleaseFile (Join-Path $Root "LICENSE") "LICENSE"
    Copy-ReleaseFile (Join-Path $Root "CHANGELOG.md") "CHANGELOG.md"

    if (Test-Path $ZipPath) {
        Remove-Item -LiteralPath $ZipPath -Force
    }
    Get-ChildItem -LiteralPath $StageDir | Compress-Archive -DestinationPath $ZipPath -Force

    Write-Host ""
    Write-Host "完成。输出文件："
    Write-Host "CLI: $CliExe"
    Write-Host "Signer: $SignerExe"
    Write-Host "Signer 启动脚本: $SignerBat"
    Write-Host "GUI: $GuiExe"
    Write-Host "Release 目录: $StageDir"
    Write-Host "Release ZIP: $ZipPath"
} finally {
    Pop-Location
}
