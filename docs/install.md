# Install RunFlow Agent

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

Each archive includes the binary, license, notice, README, and provider docs.
