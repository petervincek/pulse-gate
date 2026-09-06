use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{Local, NaiveDate};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::core::config::common::{ConfigManager, LoggingConfig};

pub struct LogManager {
    config_manager: Arc<ConfigManager>,
}

/// `LogManager` is responsible for initializing the application logger
impl LogManager {
    pub fn new(config_manager: Arc<ConfigManager>) -> Self {
        Self { config_manager }
    }

    /// Sets up file logging and returns a WorkerGuard.
    /// Crucial: The returned WorkerGuard MUST live in main.rs to keep the file thread open!
    pub fn init(&self, logging_config: &LoggingConfig) -> anyhow::Result<WorkerGuard> {
        let log_dir = self.config_manager.get_config_dir();
        fs::create_dir_all(&log_dir)?;
        prune_old_log_files(
            &log_dir,
            &logging_config.log_file_name,
            logging_config.max_log_files,
        )?;

        let log_writer: Box<dyn Write + Send> = if logging_config.max_log_file_size_mb == 0 {
            // Keep existing daily-only behavior when size cap is disabled.
            Box::new(tracing_appender::rolling::daily(
                &log_dir,
                &logging_config.log_file_name,
            ))
        } else {
            let max_file_size_bytes = mb_to_bytes(logging_config.max_log_file_size_mb);
            Box::new(DailySizeRollingFileAppender::new(
                &log_dir,
                &logging_config.log_file_name,
                max_file_size_bytes,
            )?)
        };
        let (non_blocking_writer, guard) = tracing_appender::non_blocking(log_writer);

        // build the log filter with the required log level
        // Use the normalized crate target name so `tracing` macros match the filter.
        let crate_target = env!("CARGO_PKG_NAME").replace('-', "_");
        let filter_log_level = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new(format!("{}={}", crate_target, logging_config.log_level))
        });

        let init_result = tracing_subscriber::registry()
            .with(filter_log_level)
            .with(
                fmt::layer()
                    .with_writer(non_blocking_writer)
                    .with_ansi(false),
            )
            .try_init();

        match init_result {
            Ok(_) => {}
            Err(err) => {
                let error_message = err.to_string();
                if error_message.contains("a global default trace dispatcher has already been set")
                {
                    // A global default subscriber is already set; continue with the existing one.
                } else {
                    return Err(err.into());
                }
            }
        }

        tracing::info!("Diagnostics logging framework ['tracing'] initialized successfully.");
        Ok(guard)
    }
}

struct RollingFileState {
    current_date: NaiveDate,
    current_index: u32,
    file: fs::File,
    current_size: u64,
}

struct DailySizeRollingFileAppender {
    log_dir: PathBuf,
    log_file_name: String,
    max_file_size_bytes: u64,
    state: RollingFileState,
}

impl DailySizeRollingFileAppender {
    fn new(log_dir: &Path, log_file_name: &str, max_file_size_bytes: u64) -> anyhow::Result<Self> {
        let current_date = Local::now().date_naive();
        let (file, current_index, current_size) =
            open_file_for_date(log_dir, log_file_name, current_date, max_file_size_bytes)?;

        Ok(Self {
            log_dir: log_dir.to_path_buf(),
            log_file_name: log_file_name.to_string(),
            max_file_size_bytes,
            state: RollingFileState {
                current_date,
                current_index,
                file,
                current_size,
            },
        })
    }

    fn rotate_for_date_change(&mut self) -> anyhow::Result<()> {
        let today = Local::now().date_naive();
        if today == self.state.current_date {
            return Ok(());
        }

        let (file, current_index, current_size) = open_file_for_date(
            &self.log_dir,
            &self.log_file_name,
            today,
            self.max_file_size_bytes,
        )?;
        self.state = RollingFileState {
            current_date: today,
            current_index,
            file,
            current_size,
        };

        Ok(())
    }

    fn rotate_for_size(&mut self, incoming_bytes: u64) -> anyhow::Result<()> {
        if self.max_file_size_bytes == 0 || self.state.current_size == 0 {
            return Ok(());
        }

        if self.state.current_size.saturating_add(incoming_bytes) <= self.max_file_size_bytes {
            return Ok(());
        }

        let mut next_index = self.state.current_index.saturating_add(1);
        loop {
            let next_path = log_file_path(
                &self.log_dir,
                &self.log_file_name,
                self.state.current_date,
                next_index,
            );
            let file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(next_path)?;
            let current_size = file.metadata()?.len();

            if current_size >= self.max_file_size_bytes {
                next_index = next_index.saturating_add(1);
                continue;
            }

            self.state.current_index = next_index;
            self.state.file = file;
            self.state.current_size = current_size;
            return Ok(());
        }
    }
}

impl Write for DailySizeRollingFileAppender {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.rotate_for_date_change().map_err(io::Error::other)?;
        self.rotate_for_size(buf.len() as u64)
            .map_err(io::Error::other)?;

        let bytes_written = self.state.file.write(buf)?;
        self.state.current_size = self.state.current_size.saturating_add(bytes_written as u64);
        Ok(bytes_written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.state.file.flush()
    }
}

fn mb_to_bytes(max_log_file_size_mb: usize) -> u64 {
    (max_log_file_size_mb as u64).saturating_mul(1024 * 1024)
}

fn open_file_for_date(
    log_dir: &Path,
    log_file_name: &str,
    date: NaiveDate,
    max_file_size_bytes: u64,
) -> anyhow::Result<(fs::File, u32, u64)> {
    let mut current_index = find_latest_log_file_index(log_dir, log_file_name, date)?;
    loop {
        let path = log_file_path(log_dir, log_file_name, date, current_index);
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        let current_size = file.metadata()?.len();

        if max_file_size_bytes > 0 && current_size >= max_file_size_bytes {
            current_index = current_index.saturating_add(1);
            continue;
        }

        return Ok((file, current_index, current_size));
    }
}

fn find_latest_log_file_index(
    log_dir: &Path,
    log_file_name: &str,
    date: NaiveDate,
) -> anyhow::Result<u32> {
    let mut max_index = 0;
    for entry in fs::read_dir(log_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }

        let file_name = entry.file_name().to_string_lossy().to_string();
        if let Some(index) = parse_log_file_index(&file_name, log_file_name, date) {
            max_index = max_index.max(index);
        }
    }

    Ok(max_index)
}

fn parse_log_file_index(file_name: &str, log_file_name: &str, date: NaiveDate) -> Option<u32> {
    let date_part = date.format("%Y-%m-%d").to_string();
    let prefix = format!("{}.{}", log_file_name, date_part);
    if file_name == prefix {
        return Some(0);
    }

    file_name
        .strip_prefix(&format!("{prefix}."))
        .and_then(|suffix| suffix.parse::<u32>().ok())
}

fn log_file_path(log_dir: &Path, log_file_name: &str, date: NaiveDate, index: u32) -> PathBuf {
    let date_part = date.format("%Y-%m-%d");
    let file_name = if index == 0 {
        format!("{log_file_name}.{date_part}")
    } else {
        format!("{log_file_name}.{date_part}.{index:06}")
    };

    log_dir.join(file_name)
}

fn prune_old_log_files(
    log_dir: &Path,
    log_file_name: &str,
    max_log_files: usize,
) -> anyhow::Result<()> {
    // 0 means "unlimited retention", so pruning is intentionally disabled.
    if max_log_files == 0 {
        return Ok(());
    }

    let mut log_files = collect_log_files(log_dir, log_file_name)?;
    if log_files.len() <= max_log_files {
        return Ok(());
    }

    log_files.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    let delete_count = log_files.len() - max_log_files;

    for old_file in log_files.iter().take(delete_count) {
        fs::remove_file(old_file)?;
    }

    Ok(())
}

fn collect_log_files(log_dir: &Path, log_file_name: &str) -> anyhow::Result<Vec<PathBuf>> {
    let mut log_files = Vec::new();
    for entry in fs::read_dir(log_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }

        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name.starts_with(log_file_name) {
            log_files.push(entry.path());
        }
    }

    Ok(log_files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn init_creates_log_directory_and_returns_worker_guard() {
        let temp_dir = tempdir().expect("failed to create temp dir");
        let config_dir = temp_dir.path().join("pulse-gate");
        let config_manager = Arc::new(ConfigManager::new(Some(config_dir.clone())));
        let log_manager = LogManager::new(config_manager.clone());

        let logging_config = LoggingConfig {
            log_file_name: String::from("test.log"),
            log_level: String::from("info"),
            max_log_files: 14,
            max_log_file_size_mb: 10,
        };

        let guard = log_manager.init(&logging_config).expect("init failed");

        assert!(config_dir.exists(), "log directory should be created");
        let log_files: Vec<_> = std::fs::read_dir(&config_dir)
            .expect("read log directory failed")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("test.log"))
            .collect();
        assert!(
            !log_files.is_empty(),
            "expected at least one rolling log file"
        );

        drop(guard);
    }

    #[test]
    fn init_can_be_called_twice_without_failing() {
        let temp_dir = tempdir().expect("failed to create temp dir");
        let config_dir = temp_dir.path().join("pulse-gate");
        let config_manager = Arc::new(ConfigManager::new(Some(config_dir.clone())));
        let log_manager = LogManager::new(config_manager.clone());

        let logging_config = LoggingConfig {
            log_file_name: String::from("test.log"),
            log_level: String::from("info"),
            max_log_files: 14,
            max_log_file_size_mb: 10,
        };

        let guard1 = log_manager
            .init(&logging_config)
            .expect("first init failed");
        let guard2 = log_manager
            .init(&logging_config)
            .expect("second init failed");

        assert!(config_dir.exists(), "log directory should be created");
        drop(guard1);
        drop(guard2);
    }

    #[test]
    fn prune_old_log_files_keeps_latest_entries() {
        let temp_dir = tempdir().expect("failed to create temp dir");
        let log_dir = temp_dir.path();
        let file_names = [
            "test.log.2026-01-01",
            "test.log.2026-01-02",
            "test.log.2026-01-03",
            "test.log.2026-01-04",
        ];

        for file_name in file_names {
            fs::write(log_dir.join(file_name), "x").expect("failed to create test log file");
        }

        prune_old_log_files(log_dir, "test.log", 2).expect("prune failed");

        let remaining_names: Vec<String> = fs::read_dir(log_dir)
            .expect("read dir failed")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();

        assert_eq!(remaining_names.len(), 2);
        assert!(
            remaining_names
                .iter()
                .any(|name| name == "test.log.2026-01-03")
        );
        assert!(
            remaining_names
                .iter()
                .any(|name| name == "test.log.2026-01-04")
        );
    }

    #[test]
    fn prune_old_log_files_skips_when_cap_is_zero() {
        let temp_dir = tempdir().expect("failed to create temp dir");
        let log_dir = temp_dir.path();
        fs::write(log_dir.join("test.log.2026-01-01"), "x").expect("failed to create test file");
        fs::write(log_dir.join("test.log.2026-01-02"), "x").expect("failed to create test file");

        prune_old_log_files(log_dir, "test.log", 0).expect("prune should not fail");

        let remaining_names: Vec<String> = fs::read_dir(log_dir)
            .expect("read dir failed")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();

        assert_eq!(remaining_names.len(), 2);
    }

    #[test]
    fn size_based_rotation_creates_indexed_file_for_same_day() {
        let temp_dir = tempdir().expect("failed to create temp dir");
        let mut appender =
            DailySizeRollingFileAppender::new(temp_dir.path(), "test.log", 16).expect("new failed");

        appender
            .write_all(b"1234567890")
            .expect("first write failed");
        appender
            .write_all(b"abcdefghij")
            .expect("second write failed");
        appender.flush().expect("flush failed");

        let today = Local::now().date_naive();
        let first = format!("test.log.{}", today.format("%Y-%m-%d"));
        let second = format!("test.log.{}.000001", today.format("%Y-%m-%d"));

        assert!(temp_dir.path().join(first).exists());
        assert!(temp_dir.path().join(second).exists());
    }

    #[test]
    fn parse_log_file_index_supports_base_and_indexed_names() {
        let date = NaiveDate::from_ymd_opt(2026, 7, 28).expect("invalid date");
        assert_eq!(
            parse_log_file_index("test.log.2026-07-28", "test.log", date),
            Some(0)
        );
        assert_eq!(
            parse_log_file_index("test.log.2026-07-28.000004", "test.log", date),
            Some(4)
        );
        assert_eq!(
            parse_log_file_index("test.log.2026-07-27", "test.log", date),
            None
        );
    }
}
