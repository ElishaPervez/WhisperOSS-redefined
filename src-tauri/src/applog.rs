//! Append-only diagnostic log. Event names only — NEVER transcript text,
//! audio, or key contents (spec §6).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const CAP_BYTES: u64 = 1_000_000;

pub fn format_line(unix_ms: u64, event: &str) -> String {
    format!("{unix_ms} {event}\n")
}

pub fn over_cap(len_bytes: u64) -> bool {
    len_bytes > CAP_BYTES
}

fn log_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    let dir = PathBuf::from(base).join("WhisperOSS");
    fs::create_dir_all(&dir).ok()?;
    Some(dir.join("log.txt"))
}

/// Best-effort: logging must never crash or block the pipeline.
pub fn log(event: &str) {
    let Some(path) = log_path() else { return };
    if fs::metadata(&path).map(|m| over_cap(m.len())).unwrap_or(false) {
        let _ = fs::write(&path, b"");
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(format_line(now, event).as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_has_timestamp_event_and_newline() {
        assert_eq!(format_line(1723118400123, "recording-start"),
                   "1723118400123 recording-start\n");
    }

    #[test]
    fn cap_is_one_megabyte() {
        assert!(!over_cap(1_000_000));
        assert!(over_cap(1_000_001));
    }
}
