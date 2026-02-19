//! 日志系统初始化
//! 日志文件输出到 {config_dir}/dysonbackup/log/ 目录

use anyhow::Result;
use env_logger::Builder;
use log::LevelFilter;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::sync::{Mutex, Once};

use crate::config::config_file_path;

static INIT: Once = Once::new();

/// 每次 write 后自动 flush 的包装器
struct AutoFlushWriter(Mutex<BufWriter<File>>);

impl Write for &AutoFlushWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut w = self.0.lock().unwrap();
        let n = w.write(buf)?;
        w.flush()?;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().unwrap().flush()
    }
}

/// 初始化日志系统
pub fn init_logging(debug_mode: bool) {
    INIT.call_once(|| {
        if let Err(e) = setup_logger(debug_mode) {
            eprintln!("Failed to init logger: {e}");
        }
    });
}

/// 动态切换日志级别
pub fn set_log_level(debug_mode: bool) {
    let level = if debug_mode { LevelFilter::Debug } else { LevelFilter::Error };
    log::set_max_level(level);
}

/// 获取日志目录路径
pub fn log_dir() -> Result<std::path::PathBuf> {
    let cfg_path = config_file_path()?;
    let dir = cfg_path.parent().unwrap().join("log");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn setup_logger(debug_mode: bool) -> Result<()> {
    let dir = log_dir()?;
    let filename = chrono::Local::now().format("%Y%m%d_%H%M%S.log").to_string();
    let file = File::create(dir.join(&filename))?;
    let writer = Box::leak(Box::new(AutoFlushWriter(Mutex::new(BufWriter::new(file)))));
    let level = if debug_mode { LevelFilter::Debug } else { LevelFilter::Error };

    Builder::new()
        .filter_level(level)
        .format(|buf, record| {
            writeln!(
                buf, "{} [{}] {} - {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                record.level(),
                record.target(),
                record.args()
            )
        })
        .target(env_logger::Target::Pipe(Box::new(writer as &AutoFlushWriter)))
        .init();

    Ok(())
}
