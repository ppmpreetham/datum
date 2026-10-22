use log::{Level, LevelFilter, Log, Metadata, Record};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

struct FileLogger {
    file: Mutex<File>,
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format_record(
            record.level(),
            record.target(),
            &record.args().to_string(),
            SystemTime::now(),
        );
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
        #[cfg(debug_assertions)]
        eprintln!("{line}");
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

static LOGGER: OnceLock<FileLogger> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn format_record(level: Level, target: &str, message: &str, at: SystemTime) -> String {
    let timestamp_ms = at
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let message = message.replace(['\r', '\n'], " ");
    format!("timestamp_ms={timestamp_ms} level={level} target={target} message={message}")
}

pub fn init(log_dir: &Path) -> Result<PathBuf, String> {
    if let Some(path) = LOG_PATH.get() {
        return Ok(path.clone());
    }
    fs::create_dir_all(log_dir)
        .map_err(|error| format!("Could not create backend log directory: {error}"))?;
    let path = log_dir.join("datum-backend.log");
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("Could not open backend log file: {error}"))?;
    LOGGER
        .set(FileLogger {
            file: Mutex::new(file),
        })
        .map_err(|_| "Backend logger was initialized concurrently.".to_string())?;
    log::set_logger(LOGGER.get().expect("logger was just initialized"))
        .map_err(|error| format!("Could not activate backend logger: {error}"))?;
    log::set_max_level(LevelFilter::Info);
    let _ = LOG_PATH.set(path.clone());
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn record_format_contains_timestamp_level_target_and_message() {
        let line = format_record(
            log::Level::Warn,
            "datum::calibration",
            "heartbeat lost",
            UNIX_EPOCH + Duration::from_millis(1_234),
        );
        assert_eq!(
            line,
            "timestamp_ms=1234 level=WARN target=datum::calibration message=heartbeat lost"
        );
    }
}
