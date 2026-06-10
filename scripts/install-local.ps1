param(
    [string]$InstallDir = "$env:USERPROFILE\.runflow-agent\bin",
    [switch]$SkipPath
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Push-Location $repoRoot
try {
    cargo build --release

    $source = Join-Path $repoRoot "target\release\runflow-agent.exe"
    if (-not (Test-Path $source)) {
        throw "Release binary not found: $source"
    }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item $source (Join-Path $InstallDir "runflow-agent.exe") -Force

    if (-not $SkipPath) {
        $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
        $parts = @()
        if ($currentPath) {
            $parts = $currentPath.Split(";") | Where-Object { $_ }
        }
        if ($parts -notcontains $InstallDir) {
            $newPath = if ($currentPath) { "$currentPath;$InstallDir" } else { $InstallDir }
            [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
            Write-Host "Added to user PATH: $InstallDir"
            Write-Host "Open a new terminal before running runflow-agent from PATH."
        }
    }

    & (Join-Path $InstallDir "runflow-agent.exe") --help
    Write-Host ""
    Write-Host "Installed: $(Join-Path $InstallDir 'runflow-agent.exe')"
}
finally {
    Pop-Location
}
