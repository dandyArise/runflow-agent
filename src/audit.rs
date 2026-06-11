use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::json;

pub fn record(
    command: &str,
    status: &str,
    changed_files: &[String],
    warnings: &[String],
) -> Result<(), String> {
    record_at(Path::new("."), command, status, changed_files, warnings)
}

pub(crate) fn record_at(
    root: &Path,
    command: &str,
    status: &str,
    changed_files: &[String],
    warnings: &[String],
) -> Result<(), String> {
    let dir = root.join(".flow").join("agent");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create audit directory: {e}"))?;
    let path = dir.join("audit.jsonl");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("cannot open audit file: {e}"))?;

    let line = format!(
        "{{\"timestamp\":\"{}\",\"command\":\"{}\",\"model\":\"deterministic-local\",\"status\":\"{}\",\"changed_files\":[{}],\"warnings\":[{}]}}\n",
        timestamp_seconds(),
        json::escape(command),
        json::escape(status),
        json::string_array(changed_files),
        json::string_array(warnings)
    );
    file.write_all(line.as_bytes())
        .map_err(|e| format!("cannot write audit file: {e}"))
}

pub(crate) fn timestamp_seconds() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{secs}")
}
