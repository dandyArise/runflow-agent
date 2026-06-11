param(
    [string]$ArchivePath = "",
    [string]$ExpectedVersion = "",
    [string]$InstallFromReleaseVersion = ""
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")

function Assert-True {
    param(
        [bool]$Condition,
        [string]$Message
    )
    if (-not $Condition) {
        throw $Message
    }
}

function Get-CargoVersion {
    $match = Select-String -Path (Join-Path $repoRoot "Cargo.toml") -Pattern '^version = "([^"]+)"' | Select-Object -First 1
    Assert-True ($null -ne $match) "Cannot read package version from Cargo.toml"
    return $match.Matches[0].Groups[1].Value
}

function Test-BinaryVersion {
    param(
        [string]$Binary,
        [string]$Version
    )
    $raw = & $Binary self version --format json
    Assert-True ($LASTEXITCODE -eq 0) "self version failed for $Binary"
    $versionInfo = $raw | ConvertFrom-Json
    Assert-True ($versionInfo.version -eq $Version) "Expected version $Version, got $($versionInfo.version)"
    Assert-True ($versionInfo.platform -eq "windows-x64") "Expected windows-x64 platform, got $($versionInfo.platform)"
    Assert-True ($versionInfo.asset -eq "runflow-agent-v$Version-windows-x64.zip") "Unexpected asset name $($versionInfo.asset)"
}

if (-not $ExpectedVersion) {
    $ExpectedVersion = Get-CargoVersion
}

if ($ArchivePath) {
    $archive = Resolve-Path $ArchivePath
    $extract = Join-Path $env:TEMP ("runflow-agent-release-smoke-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force -Path $extract | Out-Null
    try {
        Expand-Archive -Path $archive -DestinationPath $extract -Force
        $required = @(
            "runflow-agent.exe",
            "LICENSE.md",
            "NOTICE.md",
            "README.md",
            "providers.md",
            "install.ps1",
            "install.sh"
        )
        foreach ($name in $required) {
            $found = Get-ChildItem -Path $extract -Recurse -File | Where-Object { $_.Name -eq $name } | Select-Object -First 1
            Assert-True ($null -ne $found) "Release archive is missing $name"
        }
        $binary = Get-ChildItem -Path $extract -Recurse -File -Filter "runflow-agent.exe" | Select-Object -First 1
        Test-BinaryVersion -Binary $binary.FullName -Version $ExpectedVersion
        Write-Host "Release archive smoke test passed: $ArchivePath"
    }
    finally {
        if (Test-Path $extract) {
            Remove-Item -LiteralPath $extract -Recurse -Force
        }
    }
}

if ($InstallFromReleaseVersion) {
    $versionTag = if ($InstallFromReleaseVersion.StartsWith("v")) { $InstallFromReleaseVersion } else { "v$InstallFromReleaseVersion" }
    $version = $versionTag.TrimStart("v")
    $installDir = Join-Path $env:TEMP ("runflow-agent-install-smoke-" + [guid]::NewGuid().ToString("N"))
    try {
        & (Join-Path $repoRoot "scripts\install.ps1") -Version $versionTag -InstallDir $installDir -SkipPath
        Assert-True ($LASTEXITCODE -eq 0) "install.ps1 failed for $versionTag"
        $binary = Join-Path $installDir "runflow-agent.exe"
        Assert-True (Test-Path $binary) "Installed binary not found: $binary"
        Test-BinaryVersion -Binary $binary -Version $version
        Write-Host "Release install smoke test passed: $versionTag"
    }
    finally {
        if (Test-Path $installDir) {
            Remove-Item -LiteralPath $installDir -Recurse -Force
        }
    }
}
