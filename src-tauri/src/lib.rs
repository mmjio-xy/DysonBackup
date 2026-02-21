//! Tauri 应用入口
//! 仅包含模块声明、Tauri 命令（薄层）和 run()
//! 业务逻辑分布在各子模块中

mod config;
mod crypto;
mod logging;
mod tasks;
mod types;
mod webdav;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, State};

use config::{get_webdav_runtime_config, keyring_get, keyring_set, load_config, persist_config};
use tasks::{
    detect_windows_save_candidates, fetch_manifest, generate_backup_id, normalize_root,
    register_task, run_backup_task, run_restore_task, save_name_from_relative, unregister_task,
};
use types::{
    AppState, BackupRequest, ConfigResp, LocalSaveFile,
    RemoteBackupVersion, RestoreRequest, TaskDone, TestConnDetailResp, WebDavConfigInput,
};
use webdav::WebDavClient;

// ── 配置命令 ──────────────────────────────────────────────────

/// 读取当前配置，附带密码是否已存入 keyring 的标志
#[tauri::command]
fn get_config(state: State<'_, Arc<AppState>>) -> Result<ConfigResp, String> {
    let cfg = state.config.lock().map_err(|_| "Config lock poisoned".to_string())?;
    let password_set = cfg.webdav.as_ref()
        .map(|w| keyring_get(&w.password_secret_ref).is_ok())
        .unwrap_or(false);
    Ok(ConfigResp {
        save_root: cfg.save_root.clone(),
        save_mode: cfg.save_mode.clone(),
        save_extension: cfg.save_extension.clone(),
        webdav: cfg.webdav.clone(),
        webdav_password_set: password_set,
        save_profiles: cfg.save_profiles.clone(),
        debug_mode: cfg.debug_mode,
        encrypt_by_default: cfg.encrypt_by_default,
        encryption_password_set: keyring_get("encryption_password").is_ok(),
        close_action: cfg.close_action.clone(),
        compress_enabled: cfg.compress_enabled,
        compress_level: cfg.compress_level,
        // auto_watch: cfg.auto_watch,
    })
}

/// 切换调试模式，持久化并动态调整日志级别
#[tauri::command]
fn set_debug_mode(enabled: bool, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut cfg = state.config.lock().map_err(|_| "Config lock poisoned".to_string())?;
    cfg.debug_mode = enabled;
    persist_config(&cfg).map_err(|e| e.to_string())?;
    logging::set_log_level(enabled);
    log::info!("Debug mode set to {}", enabled);
    Ok(())
}

/// 打开开发者工具
#[tauri::command]
fn open_devtools(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        w.open_devtools();
    }
}

/// 打开日志目录
#[tauri::command]
fn open_log_dir(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = logging::log_dir().map_err(|e| e.to_string())?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

/// 获取日志目录总大小（字节）
#[tauri::command]
fn get_log_size() -> Result<u64, String> {
    let dir = logging::log_dir().map_err(|e| e.to_string())?;
    let size = walkdir::WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();
    Ok(size)
}

/// 清空日志目录中所有日志文件
#[tauri::command]
fn clear_logs() -> Result<(), String> {
    let dir = logging::log_dir().map_err(|e| e.to_string())?;
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_file() {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(())
}

/// 保存加密设置：encrypt_by_default 存 config.json，密码存 keyring
#[tauri::command]
fn save_encryption_settings(
    encrypt_by_default: bool,
    password: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut cfg = state.config.lock().map_err(|_| "Config lock poisoned".to_string())?;
    cfg.encrypt_by_default = encrypt_by_default;
    persist_config(&cfg).map_err(|e| e.to_string())?;
    if let Some(pw) = password {
        if pw.is_empty() {
            // 空密码时尝试删除 keyring 条目（忽略错误）
            let _ = keyring::Entry::new("dysonbackup", "encryption_password")
                .and_then(|e| e.delete_credential());
        } else {
            keyring_set("encryption_password", &pw).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 从 keyring 读取加密密码
#[tauri::command]
fn get_encryption_password() -> Result<String, String> {
    keyring_get("encryption_password").map_err(|e| e.to_string())
}

/// 设置存档根目录并持久化配置
#[tauri::command]
fn set_save_root(path: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let dir = PathBuf::from(&path);
    if !(dir.exists() && dir.is_dir()) {
        return Err("Save root does not exist or is not a directory".to_string());
    }
    let mut cfg = state.config.lock().map_err(|_| "Config lock poisoned".to_string())?;
    cfg.save_root = Some(path);
    persist_config(&cfg).map_err(|e| e.to_string())
}

/// 设置存档模式和扩展名并持久化
#[tauri::command]
fn set_save_settings(save_mode: String, save_extension: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut cfg = state.config.lock().map_err(|_| "Config lock poisoned".to_string())?;
    cfg.save_mode = save_mode;
    cfg.save_extension = save_extension;
    persist_config(&cfg).map_err(|e| e.to_string())
}

/// 添加存档配置
#[tauri::command]
fn add_save_profile(input: types::SaveProfileInput, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let dir = PathBuf::from(&input.save_root);
    if !dir.is_dir() { return Err("目录不存在".to_string()); }
    let mut cfg = state.config.lock().map_err(|_| "lock".to_string())?;
    if cfg.save_profiles.iter().any(|p| p.name == input.name) {
        return Err(format!("配置名 '{}' 已存在", input.name));
    }
    cfg.save_profiles.push(types::SaveProfile {
        name: input.name, save_root: input.save_root,
        save_mode: input.save_mode, save_extension: input.save_extension,
    });
    persist_config(&cfg).map_err(|e| e.to_string())
}

/// 更新存档配置
#[tauri::command]
fn update_save_profile(old_name: String, input: types::SaveProfileInput, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let dir = PathBuf::from(&input.save_root);
    if !dir.is_dir() { return Err("目录不存在".to_string()); }
    let mut cfg = state.config.lock().map_err(|_| "lock".to_string())?;
    if input.name != old_name && cfg.save_profiles.iter().any(|x| x.name == input.name) {
        return Err(format!("配置名 '{}' 已存在", input.name));
    }
    let p = cfg.save_profiles.iter_mut().find(|p| p.name == old_name)
        .ok_or_else(|| format!("配置 '{}' 不存在", old_name))?;
    p.name = input.name; p.save_root = input.save_root;
    p.save_mode = input.save_mode; p.save_extension = input.save_extension;
    persist_config(&cfg).map_err(|e| e.to_string())
}

/// 删除存档配置
#[tauri::command]
fn delete_save_profile(name: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut cfg = state.config.lock().map_err(|_| "lock".to_string())?;
    cfg.save_profiles.retain(|p| p.name != name);
    persist_config(&cfg).map_err(|e| e.to_string())
}

/// 扫描存档目录，根据 save_mode 和 save_extension 过滤
#[tauri::command]
fn scan_saves(state: State<'_, Arc<AppState>>, profile_name: Option<String>) -> Result<Vec<LocalSaveFile>, String> {
    let cfg = state.config.lock().map_err(|_| "Config lock poisoned".to_string())?;
    // 优先从 profile 查找配置
    let (root, save_mode, raw_ext) = if let Some(ref pn) = profile_name {
        let p = cfg.save_profiles.iter().find(|p| p.name == *pn)
            .ok_or_else(|| format!("Profile '{}' not found", pn))?;
        (PathBuf::from(&p.save_root), p.save_mode.clone(), p.save_extension.clone())
    } else {
        let r = if let Some(r) = cfg.save_root.clone() {
            PathBuf::from(r)
        } else {
            detect_windows_save_candidates()
                .into_iter()
                .find(|p| p.exists() && p.is_dir())
                .ok_or_else(|| "No save root found. Set one manually.".to_string())?
        };
        (r, cfg.save_mode.clone(), cfg.save_extension.clone())
    };
    drop(cfg);

    // 规范化扩展名：去掉前导 "."，统一小写
    let filter_ext = raw_ext.trim().trim_start_matches('.').to_ascii_lowercase();

    log::info!("[scan_saves] root={} mode={} ext={}", root.display(), save_mode, filter_ext);

    let mut items = Vec::new();

    if save_mode == "folder" {
        // 文件夹模式：只扫描一级子目录
        for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if !entry.file_type().map_err(|e| e.to_string())?.is_dir() { continue; }
            let path = entry.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
            let mtime = metadata.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let size: u64 = walkdir::WalkDir::new(&path).into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len()).sum();
            items.push(LocalSaveFile {
                local_file_path: path.to_string_lossy().to_string(),
                relative_path: name.clone(),
                save_name: name,
                size,
                mtime_unix: mtime,
                sha256: String::new(),
            });
        }
    } else {
        // 单文件模式
        for entry in walkdir::WalkDir::new(&root) {
            let entry = entry.map_err(|e| e.to_string())?;
            if !entry.file_type().is_file() { continue; }
            let path = entry.path();
            let ext = path.extension()
                .map(|v| v.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if ext == "tmp" || ext == "lock" { continue; }
            if !filter_ext.is_empty() && ext != filter_ext { continue; }
            let rel = path.strip_prefix(&root).map_err(|e| e.to_string())?.to_path_buf();
            let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
            let mtime = metadata.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            items.push(LocalSaveFile {
                local_file_path: path.to_string_lossy().to_string(),
                relative_path: rel.to_string_lossy().replace('\\', "/"),
                save_name: save_name_from_relative(&rel),
                size: metadata.len(),
                mtime_unix: mtime,
                sha256: String::new(),
            });
        }
    }

    items.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    log::info!("[scan_saves] found {} items", items.len());
    Ok(items)
}

// ── WebDAV 命令 ───────────────────────────────────────────────

/// 保存 WebDAV 配置，密码写入系统 keyring
#[tauri::command]
fn save_webdav_config(input: WebDavConfigInput, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    if input.base_url.trim().is_empty() || input.username.trim().is_empty() {
        return Err("baseUrl and username are required".to_string());
    }
    let url = url::Url::parse(&input.base_url).map_err(|e| e.to_string())?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("WebDAV URL must be http or https".to_string());
    }
    let host = url.host_str().unwrap_or("webdav");
    let secret_ref = format!("webdav:{}@{}", input.username, host);
    if let Some(pw) = input.password.as_deref() {
        if !pw.is_empty() {
            keyring_set(&secret_ref, pw).map_err(|e| e.to_string())?;
        }
    }
    let mut cfg = state.config.lock().map_err(|_| "Config lock poisoned".to_string())?;
    cfg.webdav = Some(types::WebDavConfig {
        base_url: input.base_url.trim().to_string(),
        username: input.username.trim().to_string(),
        password_secret_ref: secret_ref,
        remote_root: normalize_root(&input.remote_root),
    });
    persist_config(&cfg).map_err(|e| e.to_string())
}

/// 分三步测试 WebDAV 连接：服务器连通性 → 认证 → 远端目录
#[tauri::command]
async fn test_webdav_connection(state: State<'_, Arc<AppState>>) -> Result<TestConnDetailResp, String> {
    let (webdav_cfg, password) = get_webdav_runtime_config(&state).map_err(|e| e.to_string())?;
    let client = WebDavClient::new(&webdav_cfg.base_url, &webdav_cfg.username, &password)
        .map_err(|e| e.to_string())?;

    // Step 1: 服务器连通性（OPTIONS，8 秒超时）
    let (server_reachable, server_message) = match client.client
        .request(reqwest::Method::OPTIONS, client.url_for("").map_err(|e| e.to_string())?)
        .basic_auth(&client.username, Some(&client.password))
        .timeout(std::time::Duration::from_secs(8))
        .send().await
    {
        Ok(r) => (true, format!("服务器响应正常 (HTTP {})", r.status().as_u16())),
        Err(e) => {
            return Ok(TestConnDetailResp {
                server_reachable: false,
                server_message: format!("无法连接服务器: {e}"),
                auth_ok: false, auth_message: "跳过（服务器不可达）".to_string(),
                remote_dir_exists: false, remote_dir_message: "跳过（服务器不可达）".to_string(),
                overall_ok: false,
            });
        }
    };

    // Step 2: 认证检查（PROPFIND /）
    let (auth_ok, auth_message) = match client.propfind("/", 0).await {
        Ok(_) => (true, "用户名和密码验证通过".to_string()),
        Err(e) => {
            let msg = e.to_string();
            let detail = if msg.contains("401") || msg.contains("403") {
                "用户名或密码错误".to_string()
            } else {
                format!("认证检查失败: {msg}")
            };
            return Ok(TestConnDetailResp {
                server_reachable, server_message,
                auth_ok: false, auth_message: detail,
                remote_dir_exists: false, remote_dir_message: "跳过（认证失败）".to_string(),
                overall_ok: false,
            });
        }
    };

    // Step 3: 远端目录是否存在（不存在只警告，不影响 overall_ok）
    let root = normalize_root(&webdav_cfg.remote_root);
    let (remote_dir_exists, remote_dir_message) = match client.propfind(&root, 0).await {
        Ok(_) => (true, format!("远端目录 /{root} 存在")),
        Err(_) => (false, format!("远端目录 /{root} 不存在，备份时将自动创建")),
    };

    Ok(TestConnDetailResp {
        server_reachable, server_message,
        auth_ok, auth_message,
        remote_dir_exists, remote_dir_message,
        overall_ok: true,
    })
}

// ── 备份/恢复命令 ─────────────────────────────────────────────

/// 异步启动备份任务，立即返回 task_id
#[tauri::command]
async fn start_backup(app: AppHandle, state: State<'_, Arc<AppState>>, req: BackupRequest) -> Result<String, String> {
    let task_id = format!("backup_{}", generate_backup_id());
    let state_arc = state.inner().clone();
    let flag = register_task(&state_arc, &task_id);
    tauri::async_runtime::spawn({
        let app = app.clone();
        let tid = task_id.clone();
        let sc = state_arc.clone();
        async move {
            let outcome = run_backup_task(app.clone(), sc.clone(), tid.clone(), req, flag).await;
            let _ = app.emit("task_done", TaskDone {
                task_id: tid.clone(),
                success: outcome.is_ok(),
                error: outcome.err().map(|e| e.to_string()),
            });
            unregister_task(&sc, &tid);
        }
    });
    Ok(task_id)
}

/// 列出云端所有备份版本（可按 save_name 过滤）
#[tauri::command]
async fn list_remote_backups(
    state: State<'_, Arc<AppState>>,
    save_name: Option<String>,
) -> Result<Vec<RemoteBackupVersion>, String> {
    let (webdav_cfg, password) = get_webdav_runtime_config(&state).map_err(|e| e.to_string())?;
    log::info!("[list_remote_backups] base_url={} remote_root={} filter={:?}", webdav_cfg.base_url, webdav_cfg.remote_root, save_name);
    let client = WebDavClient::new(&webdav_cfg.base_url, &webdav_cfg.username, &password)
        .map_err(|e| e.to_string())?;
    let root = normalize_root(&webdav_cfg.remote_root);
    let save_names = if let Some(sn) = save_name {
        vec![sn]
    } else {
        client.list_child_dirs(&format!("{root}/v1")).await
            .map_err(|e| e.to_string())?
    };
    log::info!("[list_remote_backups] save_names={:?}", save_names);

    let mut out = Vec::new();
    for sn in save_names {
        let backups = client.list_child_dirs(&format!("{root}/v1/{sn}")).await.unwrap_or_default();
        for bid in backups {
            if let Ok(mut m) = fetch_manifest(&client, &webdav_cfg.remote_root, &sn, &bid).await {
                // 旧 manifest 迁移：补写 profile_name
                if m.profile_name.is_empty() {
                    m.profile_name = "戴森球计划".to_string();
                    let client2 = client.clone();
                    let root2 = root.clone();
                    let sn2 = sn.clone();
                    let bid2 = bid.clone();
                    let m_json = serde_json::to_vec_pretty(&m).ok();
                    if let Some(json) = m_json {
                        tauri::async_runtime::spawn(async move {
                            let path = format!("{}/v1/{}/{}/manifest.json", root2, sn2, bid2);
                            let _ = client2.put_bytes(&path, json).await;
                        });
                    }
                }
                out.push(RemoteBackupVersion {
                    save_name: sn.clone(),
                    backup_id: m.backup_id,
                    created_at: m.created_at,
                    original_size: m.original_size,
                    compressed_size: m.compressed_size,
                    encrypted: m.encrypted,
                    chunked: m.chunked,
                    compressed: m.compressed,
                    source_relative_path: m.source_relative_path,
                    profile_name: m.profile_name,
                    is_tar: m.is_tar,
                });
            }
        }
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    log::info!("[list_remote_backups] found {} backups total", out.len());
    Ok(out)
}

/// 删除云端指定备份（整个备份目录）
#[tauri::command]
async fn delete_remote_backup(
    state: State<'_, Arc<AppState>>,
    save_name: String,
    backup_id: String,
) -> Result<(), String> {
    log::info!("[delete_remote_backup] save={} backup={}", save_name, backup_id);
    let (webdav_cfg, password) = get_webdav_runtime_config(&state).map_err(|e| e.to_string())?;
    let client = WebDavClient::new(&webdav_cfg.base_url, &webdav_cfg.username, &password)
        .map_err(|e| e.to_string())?;
    let path = format!(
        "{}/v1/{}/{}",
        normalize_root(&webdav_cfg.remote_root), save_name, backup_id
    );
    client.delete(&path).await.map_err(|e| e.to_string())
}

/// 异步启动恢复任务，立即返回 task_id
#[tauri::command]
async fn start_restore(app: AppHandle, state: State<'_, Arc<AppState>>, req: RestoreRequest) -> Result<String, String> {
    let task_id = format!("restore_{}", generate_backup_id());
    let state_arc = state.inner().clone();
    let flag = register_task(&state_arc, &task_id);
    tauri::async_runtime::spawn({
        let app = app.clone();
        let tid = task_id.clone();
        let sc = state_arc.clone();
        async move {
            let outcome = run_restore_task(app.clone(), sc.clone(), tid.clone(), req, flag).await;
            let _ = app.emit("task_done", TaskDone {
                task_id: tid.clone(),
                success: outcome.is_ok(),
                error: outcome.err().map(|e| e.to_string()),
            });
            unregister_task(&sc, &tid);
        }
    });
    Ok(task_id)
}

/// 设置任务取消标志
#[tauri::command]
fn cancel_task(task_id: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let map = state.task_flags.lock().map_err(|_| "Task lock poisoned".to_string())?;
    if let Some(flag) = map.get(&task_id) {
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    } else {
        Err("Task not found".to_string())
    }
}

/// 前端响应冲突询问：overwrite / rename / cancel
#[tauri::command]
fn resolve_conflict(task_id: String, action: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let tx = state.conflict_channels.lock()
        .map_err(|_| "Lock poisoned".to_string())?
        .remove(&task_id)
        .ok_or_else(|| "No pending conflict for this task".to_string())?;
    tx.send(action).map_err(|_| "Channel closed".to_string())
}

// ── 托盘 & 文件监听命令 ──────────────────────────────────────

#[tauri::command]
fn set_close_action(action: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut cfg = state.config.lock().map_err(|_| "lock".to_string())?;
    cfg.close_action = action;
    persist_config(&cfg).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_compress_config(enabled: bool, level: i32, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut cfg = state.config.lock().map_err(|_| "lock".to_string())?;
    cfg.compress_enabled = enabled;
    cfg.compress_level = level.clamp(1, 22);
    persist_config(&cfg).map_err(|e| e.to_string())
}

/// 前端确认关闭：action = "minimize" | "quit"，remember = 是否记住
#[tauri::command]
fn confirm_close(action: String, remember: bool, app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    if remember {
        let mut cfg = state.config.lock().map_err(|_| "lock".to_string())?;
        cfg.close_action = action.clone();
        persist_config(&cfg).map_err(|e| e.to_string())?;
    }
    if action == "quit" {
        app.exit(0);
    } else if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    Ok(())
}

// #[tauri::command]
// fn set_auto_watch(enabled: bool, app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
//     let mut cfg = state.config.lock().map_err(|_| "lock".to_string())?;
//     cfg.auto_watch = enabled;
//     persist_config(&cfg).map_err(|e| e.to_string())?;
//     let save_root = cfg.save_root.clone();
//     drop(cfg);
//     if enabled {
//         if let Some(root) = save_root {
//             start_watcher(&app, &state, &root)?;
//         }
//     } else {
//         stop_watcher(&state)?;
//     }
//     Ok(())
// }
//
// #[tauri::command]
// fn start_file_watch(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
//     let root = state.config.lock().map_err(|_| "lock".to_string())?
//         .save_root.clone().ok_or("未设置存档目录")?;
//     start_watcher(&app, &state, &root)
// }
//
// #[tauri::command]
// fn stop_file_watch(state: State<'_, Arc<AppState>>) -> Result<(), String> {
//     stop_watcher(&state)
// }

// fn start_watcher(app: &AppHandle, state: &State<'_, Arc<AppState>>, root: &str) -> Result<(), String> {
//     use notify::{Config, EventKind, RecursiveMode, Watcher};
//
//     stop_watcher(state)?;
//     let path = PathBuf::from(root);
//     if !path.is_dir() { return Err("存档目录不存在".to_string()); }
//
//     let app_handle = app.clone();
//     let mut watcher = notify::RecommendedWatcher::new(
//         move |res: Result<notify::Event, notify::Error>| {
//             if let Ok(event) = res {
//                 let kind_str = match event.kind {
//                     EventKind::Create(_) => "create",
//                     EventKind::Modify(_) => "modify",
//                     EventKind::Remove(_) => "remove",
//                     _ => return,
//                 };
//                 for p in &event.paths {
//                     let _ = app_handle.emit("file_changed", FileChanged {
//                         path: p.to_string_lossy().to_string(),
//                         kind: kind_str.to_string(),
//                     });
//                 }
//             }
//         },
//         Config::default(),
//     ).map_err(|e| e.to_string())?;
//
//     watcher.watch(&path, RecursiveMode::Recursive).map_err(|e| e.to_string())?;
//     *state.file_watcher.lock().map_err(|_| "lock".to_string())? = Some(watcher);
//     log::info!("[file_watcher] watching {}", root);
//     Ok(())
// }

// fn stop_watcher(state: &State<'_, Arc<AppState>>) -> Result<(), String> {
//     let mut w = state.file_watcher.lock().map_err(|_| "lock".to_string())?;
//     if w.is_some() {
//         *w = None;
//         log::info!("[file_watcher] stopped");
//     }
//     Ok(())
// }

fn setup_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItemBuilder::with_id("show", "显示窗口").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&show, &quit]).build()?;

    TrayIconBuilder::with_id("main-tray")
        .icon(tauri::include_image!("icons/icon.ico"))
        .tooltip("戴森球计划 · 存档备份")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(w) = tray.app_handle().get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)?;
    Ok(())
}

// ── 应用入口 ──────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cfg = load_config();
    logging::init_logging(cfg.debug_mode);
    log::info!("Application starting, debug_mode={}", cfg.debug_mode);
    // let auto_watch = cfg.auto_watch;
    // let save_root = cfg.save_root.clone();
    let state = Arc::new(AppState {
        config: Mutex::new(cfg),
        task_flags: Mutex::new(HashMap::new()),
        conflict_channels: Mutex::new(HashMap::new()),
        // file_watcher: Mutex::new(None),
    });
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(move |app| {
            setup_tray(app)?;
            // // 自动启动文件监听（暂时禁用）
            // if auto_watch {
            //     if let Some(root) = &save_root {
            //         let handle = app.handle().clone();
            //         let st: State<'_, Arc<AppState>> = handle.state();
            //         let _ = start_watcher(&handle, &st, root);
            //     }
            // }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let app = window.app_handle();
                let st: State<'_, Arc<AppState>> = app.state();
                let action = st.config.lock().map(|c| c.close_action.clone()).unwrap_or_default();
                match action.as_str() {
                    "minimize" => { let _ = window.hide(); }
                    "quit" => { app.exit(0); }
                    _ => { let _ = app.emit("close_requested", ()); }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_debug_mode,
            open_devtools,
            open_log_dir,
            get_log_size,
            clear_logs,
            save_encryption_settings,
            get_encryption_password,
            set_save_root,
            set_save_settings,
            add_save_profile,
            update_save_profile,
            delete_save_profile,
            scan_saves,
            save_webdav_config,
            test_webdav_connection,
            start_backup,
            list_remote_backups,
            delete_remote_backup,
            start_restore,
            cancel_task,
            resolve_conflict,
            set_close_action,
            set_compress_config,
            confirm_close,
            // set_auto_watch,
            // start_file_watch,
            // stop_file_watch,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
