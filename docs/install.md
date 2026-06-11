# Install RunFlow Agent

## From GitHub Release

Windows:

```powershell
iwr https://github.com/dandyArise/runflow-agent/releases/latest/download/install.ps1 -UseBasicParsing | iex
```

Linux/macOS:

```sh
curl -fsSL https://github.com/dandyArise/runflow-agent/releases/latest/download/install.sh | sh
```

Default install paths:

```text
Windows: %USERPROFILE%\.runflow-agent\bin
Linux/macOS: ~/.runflow-agent/bin
```

Check the installed binary:

```powershell
runflow-agent self version
```

Update later:

```powershell
runflow-agent self update
```

Preview the update target without changing files:

```powershell
runflow-agent self update --dry-run
```

## From Source

Requires Rust stable.

```powershell
git clone https://github.com/dandyArise/runflow-agent.git
cd runflow-agent
cargo build --release
.\target\release\runflow-agent.exe --help
```

## Local Windows Install

```powershell
.\scripts\install-local.ps1
```

Default install path:

```text
%USERPROFILE%\.runflow-agent\bin
```

The script builds the release binary, copies `runflow-agent.exe`, and adds the install directory to the user PATH unless `-SkipPath` is used.

```powershell
.\scripts\install-local.ps1 -SkipPath
```

## GitHub Release

Release archives are created when a tag matching `v*` is pushed.

```powershell
git tag v0.1.0
git push origin v0.1.0
```

The release workflow builds:

- `windows-x64`
- `linux-x64`
- `macos-x64`
- `macos-arm64`

Each archive includes the binary, license, notice, README, and provider docs.

Release assets also include:

- `install.ps1`
- `install.sh`

## Release Smoke Test

Windows release archives can be checked locally:

```powershell
.\scripts\test-release-smoke.ps1 -ArchivePath .\dist\runflow-agent-v0.1.5-windows-x64.zip -ExpectedVersion 0.1.5
```

Published Windows installs can also be checked without changing PATH:

```powershell
.\scripts\test-release-smoke.ps1 -InstallFromReleaseVersion v0.1.5
```
