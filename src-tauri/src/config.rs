//! 配置文件读写与系统 Keyring 管理

use anyhow::{anyhow, Result};
use std::fs;
use std::path::PathBuf;

use crate::types::{AppConfig, AppState, WebDavConfig};

const KEYRING_SERVICE: &str = "dysonbackup";

// ── 配置文件路径 ──────────────────────────────────────────────

/// 返回配置文件路径：`{config_dir}/dysonbackup/config.json`
pub fn config_file_path() -> Result<PathBuf> {
    let base = dirs::config_dir().ok_or_else(|| anyhow!("Cannot resolve config directory"))?;
    Ok(base.join("dysonbackup").join("config.json"))
}

/// 从磁盘加载配置，失败时返回默认值（不报错）
pub fn load_config() -> AppConfig {
    let path = match config_file_path() {
        Ok(v) => v,
        Err(_) => return AppConfig::default(),
    };
    let content = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(_) => return AppConfig::default(),
    };
    serde_json::from_str(&content).unwrap_or_default()
}

/// 将配置序列化并写入磁盘，自动创建父目录
pub fn persist_config(cfg: &AppConfig) -> Result<()> {
    let path = config_file_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(cfg)?)?;
    Ok(())
}

// ── 系统 Keyring ──────────────────────────────────────────────

/// 将密码写入系统凭据管理器
pub fn keyring_set(secret_ref: &str, value: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, secret_ref)?;
    entry.set_password(value)?;
    Ok(())
}

/// 从系统凭据管理器读取密码
pub fn keyring_get(secret_ref: &str) -> Result<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, secret_ref)?;
    Ok(entry.get_password()?)
}

// ── 运行时配置获取 ────────────────────────────────────────────

/// 从 AppState 中取出 WebDAV 配置并从 keyring 读取密码
/// 返回 (WebDavConfig, password)
pub fn get_webdav_runtime_config(state: &AppState) -> Result<(WebDavConfig, String)> {
    let cfg = state
        .config
        .lock()
        .map_err(|_| anyhow!("Config lock poisoned"))?;
    let webdav = cfg
        .webdav
        .clone()
        .ok_or_else(|| anyhow!("WebDAV config is not set"))?;
    drop(cfg);
    let password = keyring_get(&webdav.password_secret_ref)?;
    Ok((webdav, password))
}
