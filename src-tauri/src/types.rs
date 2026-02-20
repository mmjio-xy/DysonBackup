//! 全局数据类型定义
//! 包含所有跨模块共享的结构体、枚举和应用状态

// use notify::RecommendedWatcher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

// ── 持久化配置 ────────────────────────────────────────────────

/// 应用主配置，序列化到 config.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub save_root: Option<String>,
    pub webdav: Option<WebDavConfig>,
    #[serde(default)]
    pub debug_mode: bool,
    #[serde(default)]
    pub encrypt_by_default: bool,
    /// "ask" | "minimize" | "quit"
    #[serde(default = "default_close_action")]
    pub close_action: String,
    // #[serde(default)]
    // pub auto_watch: bool,
}

fn default_close_action() -> String { "ask".to_string() }

/// WebDAV 连接配置（密码不存此处，存系统 keyring）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavConfig {
    pub base_url: String,
    pub username: String,
    /// keyring 中的引用键，格式：`webdav:{user}@{host}`
    pub password_secret_ref: String,
    pub remote_root: String,
}

// ── 全局运行时状态 ────────────────────────────────────────────

/// Tauri 托管的全局状态
#[derive(Default)]
pub struct AppState {
    pub config: Mutex<AppConfig>,
    /// 任务取消标志表，key 为 task_id
    pub task_flags: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// 冲突询问通道，key 为 task_id
    pub conflict_channels: Mutex<HashMap<String, oneshot::Sender<String>>>,
    // /// 文件变更监听器
    // pub file_watcher: Mutex<Option<RecommendedWatcher>>,
}

// ── Tauri 命令 I/O 类型 ───────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectPathsResp {
    pub candidates: Vec<String>,
}

/// 本地存档文件信息（sha256 在备份时才计算，扫描时为空）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSaveFile {
    pub local_file_path: String,
    pub relative_path: String,
    pub save_name: String,
    pub size: u64,
    pub mtime_unix: i64,
    pub sha256: String,
}

/// 前端提交的 WebDAV 配置（含明文密码，仅用于保存）
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavConfigInput {
    pub base_url: String,
    pub username: String,
    pub password: Option<String>,
    pub remote_root: String,
}

/// get_config 命令的响应，附带密码是否已设置的标志
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigResp {
    pub save_root: Option<String>,
    pub webdav: Option<WebDavConfig>,
    pub webdav_password_set: bool,
    pub debug_mode: bool,
    pub encrypt_by_default: bool,
    pub encryption_password_set: bool,
    pub close_action: String,
    // pub auto_watch: bool,
}

/// test_webdav_connection 的分步检测结果
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestConnDetailResp {
    pub server_reachable: bool,
    pub server_message: String,
    pub auth_ok: bool,
    pub auth_message: String,
    pub remote_dir_exists: bool,
    pub remote_dir_message: String,
    pub overall_ok: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRequest {
    pub local_file_path: String,
    pub save_name: String,
    pub source_relative_path: String,
    pub use_encryption: bool,
    pub encryption_password: Option<String>,
}

/// 文件冲突处理策略
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum ConflictPolicy {
    Ask,
    Overwrite,
    Rename,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreRequest {
    pub save_name: String,
    pub backup_id: String,
    pub target_dir: String,
    pub conflict_policy: ConflictPolicy,
    pub encryption_password: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBackupVersion {
    pub save_name: String,
    pub backup_id: String,
    pub created_at: String,
    pub original_size: u64,
    pub compressed_size: u64,
    pub encrypted: bool,
    pub chunked: bool,
    pub source_relative_path: String,
}

// ── 冲突询问事件 ─────────────────────────────────────────────

/// 后端 → 前端：发现同名文件，等待用户选择
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictFound {
    pub task_id: String,
    pub file_path: String,
}

// ── 任务进度事件 ──────────────────────────────────────────────

/// 后端 → 前端的进度推送事件
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProgress {
    pub task_id: String,
    pub phase: String,
    pub percent: u8,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub message: String,
    pub speed_bps: u64,
}

/// 任务完成事件
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDone {
    pub task_id: String,
    pub success: bool,
    pub error: Option<String>,
}

// /// 文件变更事件（后端 → 前端）
// #[derive(Debug, Clone, Serialize)]
// #[serde(rename_all = "camelCase")]
// pub struct FileChanged {
//     pub path: String,
//     pub kind: String,
// }

// ── 远端存储清单 ──────────────────────────────────────────────

/// 备份清单 v1，上传至 manifest.json
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestV1 {
    pub version: u8,
    pub backup_id: String,
    pub save_name: String,
    pub source_relative_path: String,
    pub created_at: String,
    pub original_size: u64,
    pub compressed_size: u64,
    pub encrypted: bool,
    pub encryption_meta: Option<EncryptionMeta>,
    pub chunked: bool,
    pub chunk_size: Option<u64>,
    pub chunks: Vec<ChunkMeta>,
    pub payload_sha256: String,
    pub original_sha256: String,
    pub zstd_level: i32,
}

/// AES-256-GCM 加密元数据，存入 manifest
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptionMeta {
    pub algorithm: String,
    pub kdf: String,
    pub iterations: u32,
    pub salt_b64: String,
    pub nonce_b64: String,
    /// 是否使用分段流式加密（新版备份为 true，旧版缺失视为 false）
    #[serde(default)]
    pub stream_encryption: Option<bool>,
    /// 分段大小（字节），仅 stream_encryption=true 时有效
    #[serde(default)]
    pub segment_size: Option<u64>,
}

/// 单个分片的元数据
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkMeta {
    pub index: usize,
    pub name: String,
    pub size: u64,
    pub sha256: String,
}
