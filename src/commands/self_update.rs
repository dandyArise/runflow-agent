use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::CliResult;
use crate::json;

const OWNER: &str = "dandyArise";
const REPO: &str = "runflow-agent";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug)]
struct PlatformAsset {
    platform: &'static str,
    binary: &'static str,
}

pub fn run(args: &[String]) -> Result<CliResult, String> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return Err("self requires a subcommand: version or update".to_string());
    };

    match subcommand {
        "version" => version(&args[1..]),
        "update" => update(&args[1..]),
        other => Err(format!("unknown self subcommand '{other}'")),
    }
}

fn version(args: &[String]) -> Result<CliResult, String> {
    let format_json = wants_json(args);
    let asset = platform_asset()?;
    let output = if format_json {
        format!(
            "{{\"kind\":\"self_version\",\"version\":\"{}\",\"os\":\"{}\",\"arch\":\"{}\",\"platform\":\"{}\",\"asset\":\"{}\"}}",
            json::escape(CURRENT_VERSION),
            json::escape(std::env::consts::OS),
            json::escape(std::env::consts::ARCH),
            json::escape(asset.platform),
            json::escape(&asset_name(&format!("v{CURRENT_VERSION}"), &asset))
        )
    } else {
        [
            format!("version: {CURRENT_VERSION}"),
            format!("os: {}", std::env::consts::OS),
            format!("arch: {}", std::env::consts::ARCH),
            format!("platform: {}", asset.platform),
            format!(
                "asset: {}",
                asset_name(&format!("v{CURRENT_VERSION}"), &asset)
            ),
        ]
        .join("\n")
    };

    Ok(CliResult {
        command: "self version".to_string(),
        output,
        status: "success",
        changed_files: Vec::new(),
        warnings: Vec::new(),
        audit: false,
    })
}

fn update(args: &[String]) -> Result<CliResult, String> {
    let format_json = wants_json(args);
    let dry_run = args.iter().any(|arg| arg == "--dry-run");
    let asset = platform_asset()?;
    let version = match value_after(args, "--version") {
        Some(value) => normalize_tag(value)?,
        None => latest_tag()?,
    };
    let install_dir = value_after(args, "--install-dir")
        .map(PathBuf::from)
        .unwrap_or_else(default_install_dir);
    let target = install_dir.join(asset.binary);
    let url = asset_url(&version, &asset);

    if dry_run {
        return Ok(update_result(
            &version,
            &asset,
            &url,
            &install_dir,
            &target,
            "dry-run",
            format_json,
            false,
        ));
    }

    let work_dir = std::env::temp_dir().join(format!(
        "runflow-agent-update-{}-{}",
        version.trim_start_matches('v'),
        std::process::id()
    ));
    let extract_dir = work_dir.join("extract");
    fs::create_dir_all(&extract_dir).map_err(|e| {
        format!(
            "cannot create update temp dir '{}': {e}",
            extract_dir.display()
        )
    })?;
    let zip_path = work_dir.join(asset_name(&version, &asset));

    download_file(&url, &zip_path)?;
    extract_zip(&zip_path, &extract_dir)?;
    let source = find_extracted_binary(&extract_dir, asset.binary).ok_or_else(|| {
        format!(
            "downloaded archive did not contain expected binary '{}'",
            asset.binary
        )
    })?;

    fs::create_dir_all(&install_dir)
        .map_err(|e| format!("cannot create install dir '{}': {e}", install_dir.display()))?;
    let staged = install_binary(&source, &target)?;
    let status = if staged { "staged" } else { "updated" };
    Ok(update_result(
        &version,
        &asset,
        &url,
        &install_dir,
        &target,
        status,
        format_json,
        true,
    ))
}

fn update_result(
    version: &str,
    asset: &PlatformAsset,
    url: &str,
    install_dir: &Path,
    target: &Path,
    status: &'static str,
    format_json: bool,
    changed: bool,
) -> CliResult {
    let output = if format_json {
        format!(
            "{{\"kind\":\"self_update\",\"status\":\"{}\",\"version\":\"{}\",\"platform\":\"{}\",\"url\":\"{}\",\"install_dir\":\"{}\",\"binary\":\"{}\"}}",
            json::escape(status),
            json::escape(version),
            json::escape(asset.platform),
            json::escape(url),
            json::escape(&install_dir.display().to_string()),
            json::escape(&target.display().to_string())
        )
    } else {
        [
            format!("status: {status}"),
            format!("version: {version}"),
            format!("platform: {}", asset.platform),
            format!("url: {url}"),
            format!("install_dir: {}", install_dir.display()),
            format!("binary: {}", target.display()),
        ]
        .join("\n")
    };

    CliResult {
        command: "self update".to_string(),
        output,
        status: "success",
        changed_files: if changed {
            vec![target.display().to_string()]
        } else {
            Vec::new()
        },
        warnings: Vec::new(),
        audit: true,
    }
}

fn latest_tag() -> Result<String, String> {
    let url = format!("https://api.github.com/repos/{OWNER}/{REPO}/releases/latest");
    let text = download_text(&url)?;
    parse_latest_tag(&text).ok_or_else(|| "cannot find latest release tag".to_string())
}

fn parse_latest_tag(text: &str) -> Option<String> {
    let marker = "\"tag_name\"";
    let after = text.split(marker).nth(1)?;
    let after_colon = after.split_once(':')?.1.trim_start();
    let tag = after_colon.strip_prefix('"')?.split('"').next()?;
    if tag.is_empty() {
        None
    } else {
        Some(tag.to_string())
    }
}

fn normalize_tag(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("--version cannot be empty".to_string());
    }
    if trimmed.starts_with('v') {
        Ok(trimmed.to_string())
    } else {
        Ok(format!("v{trimmed}"))
    }
}

fn platform_asset() -> Result<PlatformAsset, String> {
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => PlatformAsset {
            platform: "windows-x64",
            binary: "runflow-agent.exe",
        },
        ("linux", "x86_64") => PlatformAsset {
            platform: "linux-x64",
            binary: "runflow-agent",
        },
        ("macos", "x86_64") => PlatformAsset {
            platform: "macos-x64",
            binary: "runflow-agent",
        },
        (os, arch) => return Err(format!("unsupported self-update platform: {os}-{arch}")),
    };
    Ok(platform)
}

fn asset_name(version: &str, asset: &PlatformAsset) -> String {
    format!("{REPO}-{version}-{}.zip", asset.platform)
}

fn asset_url(version: &str, asset: &PlatformAsset) -> String {
    format!(
        "https://github.com/{OWNER}/{REPO}/releases/download/{version}/{}",
        asset_name(version, asset)
    )
}

fn default_install_dir() -> PathBuf {
    if let Ok(current) = std::env::current_exe() {
        let current_text = current.to_string_lossy().replace('\\', "/");
        if !current_text.contains("/target/debug/") && !current_text.contains("/target/release/") {
            if let Some(parent) = current.parent() {
                return parent.to_path_buf();
            }
        }
    }
    home_dir().join(".runflow-agent").join("bin")
}

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn download_text(url: &str) -> Result<String, String> {
    if cfg!(windows) {
        run_capture(
            "powershell",
            &[
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &format!(
                    "(Invoke-WebRequest -Headers @{{'User-Agent'='runflow-agent'}} -Uri {} -UseBasicParsing).Content",
                    ps_quote(url)
                ),
            ],
        )
    } else {
        run_capture("curl", &["-fsSL", "-H", "User-Agent: runflow-agent", url])
    }
}

fn download_file(url: &str, path: &Path) -> Result<(), String> {
    if cfg!(windows) {
        run_status(
            "powershell",
            &[
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &format!(
                    "Invoke-WebRequest -Headers @{{'User-Agent'='runflow-agent'}} -Uri {} -OutFile {} -UseBasicParsing",
                    ps_quote(url),
                    ps_quote(&path.display().to_string())
                ),
            ],
        )
    } else {
        run_status(
            "curl",
            &[
                "-fL",
                "-H",
                "User-Agent: runflow-agent",
                "-o",
                &path.display().to_string(),
                url,
            ],
        )
    }
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    if cfg!(windows) {
        run_status(
            "powershell",
            &[
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &format!(
                    "Expand-Archive -Path {} -DestinationPath {} -Force",
                    ps_quote(&zip_path.display().to_string()),
                    ps_quote(&dest.display().to_string())
                ),
            ],
        )
    } else if command_exists("unzip") {
        run_status(
            "unzip",
            &[
                "-q",
                "-o",
                &zip_path.display().to_string(),
                "-d",
                &dest.display().to_string(),
            ],
        )
    } else {
        run_status(
            "python3",
            &[
                "-c",
                "import sys,zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])",
                &zip_path.display().to_string(),
                &dest.display().to_string(),
            ],
        )
    }
}

fn install_binary(source: &Path, target: &Path) -> Result<bool, String> {
    if cfg!(windows) && is_current_exe(target) {
        stage_windows_replace(source, target)?;
        return Ok(true);
    }
    fs::copy(source, target).map_err(|e| {
        format!(
            "cannot install '{}' to '{}': {e}",
            source.display(),
            target.display()
        )
    })?;
    if !cfg!(windows) {
        let _ = run_status("chmod", &["+x", &target.display().to_string()]);
    }
    Ok(false)
}

fn find_extracted_binary(root: &Path, binary: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.eq_ignore_ascii_case(binary))
                .unwrap_or(false)
        {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_extracted_binary(&path, binary) {
                return Some(found);
            }
        }
    }
    None
}

fn stage_windows_replace(source: &Path, target: &Path) -> Result<(), String> {
    let script =
        std::env::temp_dir().join(format!("runflow-agent-update-{}.ps1", std::process::id()));
    let body = format!(
        "$ErrorActionPreference='Stop'\nWait-Process -Id {} -Timeout 60 -ErrorAction SilentlyContinue\nNew-Item -ItemType Directory -Force -Path (Split-Path -Parent {}) | Out-Null\nCopy-Item -Force -Path {} -Destination {}\n",
        std::process::id(),
        ps_quote(&target.display().to_string()),
        ps_quote(&source.display().to_string()),
        ps_quote(&target.display().to_string())
    );
    fs::write(&script, body)
        .map_err(|e| format!("cannot write updater script '{}': {e}", script.display()))?;
    Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-File",
            &script.display().to_string(),
        ])
        .spawn()
        .map_err(|e| format!("cannot start updater script: {e}"))?;
    Ok(())
}

fn is_current_exe(path: &Path) -> bool {
    let Ok(current) = std::env::current_exe() else {
        return false;
    };
    normalize_path(&current) == normalize_path(path)
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

fn command_exists(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .output()
        .map(|output| output.status.success() || !output.stdout.is_empty())
        .unwrap_or(false)
}

fn run_capture(command: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(command)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run {command}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{command} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_status(command: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(command)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run {command}: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{command} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn wants_json(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--format") && args.iter().any(|arg| arg == "json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_version_tags() {
        assert_eq!(normalize_tag("0.1.3").unwrap(), "v0.1.3");
        assert_eq!(normalize_tag("v0.1.3").unwrap(), "v0.1.3");
    }

    #[test]
    fn parses_latest_release_tag() {
        let tag = parse_latest_tag("{\"url\":\"x\",\"tag_name\":\"v0.1.3\"}").unwrap();
        assert_eq!(tag, "v0.1.3");
    }

    #[test]
    fn builds_asset_url() {
        let asset = PlatformAsset {
            platform: "windows-x64",
            binary: "runflow-agent.exe",
        };
        assert_eq!(
            asset_url("v0.1.3", &asset),
            "https://github.com/dandyArise/runflow-agent/releases/download/v0.1.3/runflow-agent-v0.1.3-windows-x64.zip"
        );
    }

    #[test]
    fn finds_binary_in_flat_or_nested_extract() {
        let root = std::env::temp_dir().join(format!(
            "runflow-agent-find-bin-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let nested = root.join("pkg");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("runflow-agent.exe"), "x").unwrap();

        let found = find_extracted_binary(&root, "runflow-agent.exe").unwrap();

        fs::remove_dir_all(root).unwrap();
        assert!(found.ends_with("runflow-agent.exe"));
    }

    #[test]
    fn self_version_returns_json() {
        let result = version(&["--format".to_string(), "json".to_string()]).unwrap();
        assert_eq!(result.status, "success");
        assert!(result.output.contains("\"kind\":\"self_version\""));
    }
}
