//! 配置文件读写与系统 Keyring 管理

use anyhow::{anyhow, Result};
use std::fs;
use std::path::PathBuf;

use crate::types::{AppConfig, AppState, LocalSyncConfig, WebDavConfig};

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
    let mut cfg: AppConfig = serde_json::from_str(&content).unwrap_or_default();
    // 旧配置迁移：若无 profiles 但有 save_root，自动创建默认 profile
    if cfg.save_profiles.is_empty() {
        if let Some(ref root) = cfg.save_root {
            if !root.is_empty() {
                cfg.save_profiles.push(crate::types::SaveProfile {
                    name: "戴森球计划".to_string(),
                    save_root: root.clone(),
                    save_mode: cfg.save_mode.clone(),
                    save_extension: cfg.save_extension.clone(),
                });
                let _ = persist_config(&cfg);
            }
        }
    }
    cfg
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

// ── 本地同步配置 ──────────────────────────────────────────────

fn local_sync_path() -> Result<PathBuf> {
    let base = dirs::config_dir().ok_or_else(|| anyhow!("Cannot resolve config directory"))?;
    Ok(base.join("dysonbackup").join("local_sync.json"))
}

pub fn load_local_sync() -> LocalSyncConfig {
    local_sync_path()
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn persist_local_sync(cfg: &LocalSyncConfig) -> Result<()> {
    let path = local_sync_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(cfg)?)?;
    Ok(())
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
