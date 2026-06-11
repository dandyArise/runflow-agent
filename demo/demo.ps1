param(
    [string]$Binary = ""
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")

function Write-Utf8NoBom {
    param(
        [string]$Path,
        [string]$Content
    )

    [System.IO.File]::WriteAllText((Join-Path (Get-Location) $Path), $Content, [System.Text.UTF8Encoding]::new($false))
}

if (-not $Binary) {
    $releaseBinary = Join-Path $repoRoot "target\release\runflow-agent.exe"
    $debugBinary = Join-Path $repoRoot "target\debug\runflow-agent.exe"
    if (Test-Path $releaseBinary) {
        $Binary = $releaseBinary
    }
    elseif (Test-Path $debugBinary) {
        $Binary = $debugBinary
    }
    else {
        Push-Location $repoRoot
        cargo build
        Pop-Location
        $Binary = $debugBinary
    }
}
else {
    $Binary = (Resolve-Path $Binary).Path
}

$workspace = Join-Path ([System.IO.Path]::GetTempPath()) ("runflow-agent-demo-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $workspace | Out-Null

try {
    Push-Location $workspace

    New-Item -ItemType Directory -Force -Path ".flow\jobs" | Out-Null
    New-Item -ItemType Directory -Force -Path ".flow\runs\demo-failed-run" | Out-Null
    New-Item -ItemType Directory -Force -Path ".flow\agent\drafts" | Out-Null
    New-Item -ItemType Directory -Force -Path "logs\demo-failed-run\build" | Out-Null
    $draftPath = ".flow\agent\drafts\ping-workflow.yml"

    Write-Utf8NoBom ".flow\jobs\ping.yml" @'
name: ping
version: 1
steps:
  - id: ping
    type: shell
    command: ping
    args: ["1.1.1.1"]
'@

    Write-Utf8NoBom ".flow\runs\demo-failed-run\manifest.json" @'
{"job_name":"ping","status":"FAILED","failed_step":"build","exit_code":127,"started_at":"2026-06-11T10:00:00Z","ended_at":"2026-06-11T10:01:00Z"}
'@

    Write-Utf8NoBom ".flow\runs\demo-failed-run\events.jsonl" @'
{"event":"run_started","status":"RUNNING"}
{"event":"step_started","step":"build","status":"RUNNING"}
{"event":"step_failed","step":"build","status":"FAILED","message":"command failed"}
'@

    Write-Utf8NoBom "logs\demo-failed-run\build\step.metadata.json" @'
{"step":"build","exitCode":127}
'@

    Write-Utf8NoBom "logs\demo-failed-run\build\stdout.log" "installing dependencies"
    Write-Utf8NoBom "logs\demo-failed-run\build\stderr.log" "command not found: cargo-nextest"

    Write-Host "== RunFlow Agent demo workspace =="
    Write-Host $workspace
    Write-Host ""

    Write-Host "== Draft workflow =="
    $draftOutput = & $Binary draft --prompt "Ping 1.1.1.1 every 5 minutes" --output $draftPath
    $draftOutput | Write-Host
    Write-Host ""

    Write-Host "== Review workflow =="
    $reviewOutput = & $Binary review $draftPath
    $reviewJson = & $Binary review $draftPath --format json
    $reviewOutput | Write-Host
    Write-Host ""

    Write-Host "== Inspect workspace =="
    $inspectOutput = & $Binary inspect-workspace
    $inspectJson = & $Binary inspect-workspace --format json
    $inspectOutput | Write-Host
    Write-Host ""

    Write-Host "== Explain failed run =="
    $explainOutput = & $Binary explain-run "demo-failed-run"
    $explainJson = & $Binary explain-run "demo-failed-run" --format json
    $explainOutput | Write-Host
    Write-Host ""

    Write-Host "== Daily report =="
    $reportJson = & $Binary report daily --format json
    $reportOutput = & $Binary report daily
    $reportJson | Write-Host
    Write-Host ""

    $draftText = $draftOutput -join "`n"
    $reviewText = $reviewOutput -join "`n"
    $inspectText = $inspectOutput -join "`n"
    $explainText = $explainOutput -join "`n"
    $reportText = $reportOutput -join "`n"

    $resultText = @"
== Draft workflow ==
$draftText

== Review workflow ==
$reviewText

== Inspect workspace ==
$inspectText

== Explain failed run ==
$explainText

== Daily report ==
$reportText
"@

    Write-Utf8NoBom "demo-result.txt" $resultText
    Write-Utf8NoBom "demo-review.json" ($reviewJson -join "`n")
    Write-Utf8NoBom "demo-inspect.json" ($inspectJson -join "`n")
    Write-Utf8NoBom "demo-explain.json" ($explainJson -join "`n")
    Write-Utf8NoBom "demo-report.json" ($reportJson -join "`n")

    Write-Host "Demo files kept in:"
    Write-Host $workspace
    Write-Host ""
    Write-Host "Demo results:"
    Write-Host (Join-Path $workspace "demo-result.txt")
    Write-Host (Join-Path $workspace "demo-inspect.json")
    Write-Host (Join-Path $workspace "demo-explain.json")
    Write-Host (Join-Path $workspace "demo-report.json")
}
finally {
    Pop-Location
}
