param(
  [string]$Version = "latest",
  [string]$InstallDir = "$env:USERPROFILE\.runflow-agent\bin",
  [switch]$SkipPath
)

$ErrorActionPreference = "Stop"
$repo = "dandyArise/runflow-agent"

if ($Version -eq "latest") {
  $release = Invoke-RestMethod -Headers @{ "User-Agent" = "runflow-agent-installer" } -Uri "https://api.github.com/repos/$repo/releases/latest"
  $Version = $release.tag_name
}
if (-not $Version.StartsWith("v")) {
  $Version = "v$Version"
}

$asset = "runflow-agent-$Version-windows-x64.zip"
$url = "https://github.com/$repo/releases/download/$Version/$asset"
$work = Join-Path $env:TEMP "runflow-agent-install-$Version"
$zip = Join-Path $work $asset
$extract = Join-Path $work "extract"

New-Item -ItemType Directory -Force -Path $work, $extract, $InstallDir | Out-Null
Invoke-WebRequest -Headers @{ "User-Agent" = "runflow-agent-installer" } -Uri $url -OutFile $zip -UseBasicParsing
Expand-Archive -Path $zip -DestinationPath $extract -Force
$binary = Get-ChildItem -Path $extract -Recurse -Filter "runflow-agent.exe" | Select-Object -First 1
if (-not $binary) {
  throw "Archive did not contain runflow-agent.exe"
}
Copy-Item -Force -Path $binary.FullName -Destination (Join-Path $InstallDir "runflow-agent.exe")

if (-not $SkipPath) {
  $path = [Environment]::GetEnvironmentVariable("Path", "User")
  $parts = @($path -split ";") | Where-Object { $_ -ne "" }
  if ($parts -notcontains $InstallDir) {
    $next = if ([string]::IsNullOrWhiteSpace($path)) { $InstallDir } else { "$path;$InstallDir" }
    [Environment]::SetEnvironmentVariable("Path", $next, "User")
    Write-Host "Added to user PATH. Open a new terminal before using runflow-agent globally."
  }
}

$exe = Join-Path $InstallDir "runflow-agent.exe"
& $exe self version 2>$null
if ($LASTEXITCODE -ne 0) {
  & $exe --help | Select-Object -First 3
}
