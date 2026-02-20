//! 备份与恢复任务的核心逻辑
//! 包含任务注册/取消、进度推送、备份/恢复流程

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use log::{debug, info};
use rand::distr::Alphanumeric;
use rand::{rng, Rng};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufReader, Read as _, Write as _};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

use crate::config::get_webdav_runtime_config;
use crate::crypto::{decrypt_aes_gcm, decrypt_aes_gcm_stream, encrypt_aes_gcm_stream, sha256_hex};
use crate::types::{
    AppState, BackupRequest, ChunkMeta, ConflictFound, ConflictPolicy, ManifestV1,
    RestoreRequest, TaskProgress,
};
use crate::webdav::WebDavClient;

/// 大文件分片阈值：超过此大小则切片上传
pub const SPLIT_THRESHOLD_BYTES: usize = 100 * 1024 * 1024;
/// 每个分片的大小
pub const CHUNK_SIZE_BYTES: usize = 10 * 1024 * 1024;

// ── 任务管理 ──────────────────────────────────────────────────

/// 注册任务，返回取消标志（AtomicBool）
pub fn register_task(state: &AppState, task_id: &str) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut map) = state.task_flags.lock() {
        map.insert(task_id.to_string(), flag.clone());
    }
    flag
}

/// 任务完成后从状态表中移除
pub fn unregister_task(state: &AppState, task_id: &str) {
    if let Ok(mut map) = state.task_flags.lock() {
        map.remove(task_id);
    }
}

/// 检查取消标志，若已设置则返回 Err
pub fn should_cancel(flag: &Arc<AtomicBool>) -> Result<()> {
    if flag.load(Ordering::Relaxed) {
        Err(anyhow!("Task cancelled"))
    } else {
        Ok(())
    }
}

// ── 进度推送 ──────────────────────────────────────────────────

/// 向前端发送 task_progress 事件
#[allow(clippy::too_many_arguments)]
pub fn emit_progress(
    app: &AppHandle,
    task_id: &str,
    phase: &str,
    percent: u8,
    bytes_done: u64,
    bytes_total: u64,
    message: &str,
    speed_bps: u64,
) {
    let _ = app.emit("task_progress", TaskProgress {
        task_id: task_id.to_string(),
        phase: phase.to_string(),
        percent,
        bytes_done,
        bytes_total,
        message: message.to_string(),
        speed_bps,
    });
}

// ── 工具函数 ──────────────────────────────────────────────────

/// 生成唯一备份 ID：`{时间戳}_{6位随机字母数字}`
pub fn generate_backup_id() -> String {
    let ts = Utc::now().format("%Y%m%dT%H%M%SZ");
    let suffix: String = rng()
        .sample_iter(&Alphanumeric)
        .take(6)
        .map(char::from)
        .collect::<String>()
        .to_lowercase();
    format!("{ts}_{suffix}")
}

/// 去除路径首尾的斜杠和反斜杠
pub fn normalize_root(input: &str) -> String {
    input.trim().trim_matches('/').trim_matches('\\').to_string()
}

/// 从相对路径中提取第一级目录名作为存档名
pub fn save_name_from_relative(rel: &Path) -> String {
    rel.components()
        .next()
        .and_then(|c| match c {
            Component::Normal(v) => Some(v.to_string_lossy().to_string()),
            _ => None,
        })
        .unwrap_or_else(|| "default".to_string())
}

/// 校验相对路径安全性，拒绝绝对路径和路径穿越
pub fn sanitize_relative_path(value: &str) -> Result<PathBuf> {
    let candidate = PathBuf::from(value);
    if candidate.is_absolute() {
        return Err(anyhow!("Relative path must not be absolute"));
    }
    for c in candidate.components() {
        if matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_)) {
            return Err(anyhow!("Invalid relative path"));
        }
    }
    Ok(candidate)
}

/// 生成带时间戳的重命名路径，避免覆盖
fn rename_path(desired: &Path, fallback_parent: &Path) -> PathBuf {
    let now = Utc::now().format("%Y%m%d%H%M%S");
    let stem = desired.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "restored".to_string());
    let ext = desired.extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let parent = desired.parent().unwrap_or(fallback_parent);
    parent.join(format!("{stem}_restored_{now}{ext}"))
}

/// 探测 Windows 上戴森球计划的默认存档目录
pub fn detect_windows_save_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(docs) = dirs::document_dir() {
        paths.push(docs.join("Dyson Sphere Program").join("Save"));
        paths.push(docs.join("DysonSphereProgram").join("Save"));
    }
    paths
}

// ── 备份任务 ──────────────────────────────────────────────────

/// 执行完整备份流程：流式读文件+SHA256 → 流式压缩 → 分段加密（可选）→ 上传 → 写 manifest
pub async fn run_backup_task(
    app: AppHandle,
    state: Arc<AppState>,
    task_id: String,
    req: BackupRequest,
    flag: Arc<AtomicBool>,
) -> Result<()> {
    info!("Backup task {} started: file={} save_name={} encrypt={} compress={}", task_id, req.local_file_path, req.save_name, req.use_encryption, req.use_compression);

    // 1+2. 流式读文件 → 增量 SHA256 → 可选 zstd 流式压缩
    should_cancel(&flag)?;

    let file = File::open(&req.local_file_path)
        .with_context(|| format!("Failed to open file: {}", req.local_file_path))?;
    let original_size = file.metadata()?.len();
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut hasher = Sha256::new();
    let mut payload_data = Vec::new();
    let zstd_level = if req.use_compression { req.compression_level } else { 0 };

    // 读取文件 + SHA256
    emit_progress(&app, &task_id, "read", 5, 0, 0, "Reading file", 0);
    let mut raw_data = Vec::new();
    {
        let mut buf = [0u8; 64 * 1024];
        let mut bytes_read = 0u64;
        loop {
            should_cancel(&flag)?;
            let n = reader.read(&mut buf)?;
            if n == 0 { break; }
            hasher.update(&buf[..n]);
            raw_data.extend_from_slice(&buf[..n]);
            bytes_read += n as u64;
            let pct = 5 + ((bytes_read as f64 / original_size.max(1) as f64) * 15.0) as u8;
            emit_progress(&app, &task_id, "read", pct.min(20), bytes_read, original_size, "Reading file", 0);
        }
    }

    // 压缩（可选）
    if req.use_compression {
        emit_progress(&app, &task_id, "compress", 22, 0, 0, "Compressing", 0);
        let mut encoder = zstd::Encoder::new(&mut payload_data, req.compression_level)?;
        encoder.write_all(&raw_data)?;
        encoder.finish()?;
        emit_progress(&app, &task_id, "compress", 30, 0, 0, "Compressed", 0);
    } else {
        payload_data = raw_data;
    }

    let original_sha = hex::encode(hasher.finalize());
    info!("[backup:{}] streamed {} → {} bytes, sha256={}", task_id, original_size, payload_data.len(), original_sha);

    // 3. 分段 AES-256-GCM 加密（可选）
    should_cancel(&flag)?;
    let mut payload = payload_data;
    let mut encryption_meta = None;
    if req.use_encryption {
        let password = req.encryption_password.as_deref()
            .ok_or_else(|| anyhow!("Encryption password is required"))?;
        emit_progress(&app, &task_id, "encrypt", 35, 0, payload.len() as u64, "Encrypting payload", 0);
        let (cipher, meta) = encrypt_aes_gcm_stream(password, &payload)?;
        payload = cipher;
        encryption_meta = Some(meta);
        info!("[backup:{}] stream-encrypted, payload now {} bytes", task_id, payload.len());
    }

    // 4. 创建远端目录并上传（大文件分片，小文件整体）
    let (webdav_cfg, webdav_password) = get_webdav_runtime_config(&state)?;
    info!("[backup:{}] webdav base_url={} remote_root={}", task_id, webdav_cfg.base_url, webdav_cfg.remote_root);
    let client = WebDavClient::new(&webdav_cfg.base_url, &webdav_cfg.username, &webdav_password)?;
    let backup_id = generate_backup_id();
    let remote_prefix = format!(
        "{}/v1/{}/{}",
        normalize_root(&webdav_cfg.remote_root), req.save_name, backup_id
    );
    info!("[backup:{}] remote_prefix={} chunked={} payload_size={}", task_id, remote_prefix, payload.len() > SPLIT_THRESHOLD_BYTES, payload.len());
    client.mkcol_recursive(&remote_prefix).await?;

    let payload_sha = sha256_hex(&payload);
    let mut chunks = Vec::new();
    let chunked = payload.len() > SPLIT_THRESHOLD_BYTES;

    if chunked {
        // 分片上传
        client.mkcol_recursive(&format!("{remote_prefix}/chunks")).await?;
        let total = payload.len() as u64;
        let upload_start = Instant::now();
        let mut uploaded_so_far = 0u64;
        for (index, chunk) in payload.chunks(CHUNK_SIZE_BYTES).enumerate() {
            should_cancel(&flag)?;
            let chunk_name = format!("chunk_{:06}.part", index + 1);
            let chunk_len = chunk.len() as u64;
            let base_uploaded = uploaded_so_far;
            let app2 = app.clone();
            let tid2 = task_id.clone();
            let t0 = upload_start;
            client.put_bytes_with_progress(
                &format!("{remote_prefix}/chunks/{chunk_name}"),
                chunk.to_vec(),
                flag.clone(),
                move |done, _chunk_total| {
                    let global_done = base_uploaded + done;
                    let pct = 40 + ((global_done as f64 / total as f64) * 50.0) as u8;
                    let elapsed = t0.elapsed().as_secs_f64().max(0.001);
                    let speed = (global_done as f64 / elapsed) as u64;
                    emit_progress(&app2, &tid2, "upload", pct.min(92), global_done, total, "Uploading", speed);
                },
            ).await?;
            uploaded_so_far += chunk_len;
            chunks.push(ChunkMeta {
                index,
                name: chunk_name,
                size: chunk.len() as u64,
                sha256: sha256_hex(chunk),
            });
        }
    } else {
        // 整体上传
        let upload_start = Instant::now();
        let app2 = app.clone();
        let tid2 = task_id.clone();
        client.put_bytes_with_progress(
            &format!("{remote_prefix}/payload.bin"),
            payload.clone(),
            flag.clone(),
            move |done, total| {
                let pct = 40 + ((done as f64 / total.max(1) as f64) * 50.0) as u8;
                let elapsed = upload_start.elapsed().as_secs_f64().max(0.001);
                let speed = (done as f64 / elapsed) as u64;
                emit_progress(&app2, &tid2, "upload", pct.min(92), done, total, "Uploading", speed);
            },
        ).await?;
    }

    // 5. 上传 manifest.json
    let manifest = ManifestV1 {
        version: 1,
        backup_id,
        save_name: req.save_name,
        source_relative_path: req.source_relative_path,
        created_at: Utc::now().to_rfc3339(),
        original_size,
        compressed_size: payload.len() as u64,
        encrypted: encryption_meta.is_some(),
        encryption_meta,
        chunked,
        chunk_size: if chunked { Some(CHUNK_SIZE_BYTES as u64) } else { None },
        chunks,
        payload_sha256: payload_sha,
        original_sha256: original_sha,
        zstd_level,
        compressed: req.use_compression,
    };
    emit_progress(&app, &task_id, "manifest", 95, 0, 0, "Uploading manifest", 0);
    let manifest_json = serde_json::to_vec_pretty(&manifest)?;
    info!("[backup:{}] uploading manifest ({} bytes): {}", task_id, manifest_json.len(), String::from_utf8_lossy(&manifest_json));
    client.put_bytes(
        &format!("{remote_prefix}/manifest.json"),
        manifest_json,
    ).await?;

    info!("[backup:{}] completed successfully", task_id);
    emit_progress(&app, &task_id, "done", 100, 0, 0, "Backup completed", 0);
    Ok(())
}

// ── 恢复任务 ──────────────────────────────────────────────────

/// 下载 manifest.json 并解析
pub async fn fetch_manifest(
    client: &WebDavClient,
    remote_root: &str,
    save_name: &str,
    backup_id: &str,
) -> Result<ManifestV1> {
    let path = format!("{}/v1/{}/{}/manifest.json", normalize_root(remote_root), save_name, backup_id);
    debug!("fetch_manifest path={}", path);
    let bytes = client.get_bytes(&path).await?;
    debug!("fetch_manifest got {} bytes", bytes.len());
    Ok(serde_json::from_slice(&bytes)?)
}

/// 执行完整恢复流程：下载 → 校验 → 解密（可选）→ 解压 → 写文件
pub async fn run_restore_task(
    app: AppHandle,
    state: Arc<AppState>,
    task_id: String,
    req: RestoreRequest,
    flag: Arc<AtomicBool>,
) -> Result<()> {
    info!("Restore task {} started: save={} backup={} target_dir={}", task_id, req.save_name, req.backup_id, req.target_dir);
    should_cancel(&flag)?;
    let (webdav_cfg, webdav_password) = get_webdav_runtime_config(&state)?;
    info!("[restore:{}] webdav base_url={} remote_root={}", task_id, webdav_cfg.base_url, webdav_cfg.remote_root);
    let client = WebDavClient::new(&webdav_cfg.base_url, &webdav_cfg.username, &webdav_password)?;

    // 1. 下载并解析 manifest
    emit_progress(&app, &task_id, "manifest", 10, 0, 0, "Downloading manifest", 0);
    let manifest = fetch_manifest(&client, &webdav_cfg.remote_root, &req.save_name, &req.backup_id).await?;
    info!("[restore:{}] manifest: original_size={} compressed={} encrypted={} chunked={} chunks={}",
        task_id, manifest.original_size, manifest.compressed_size, manifest.encrypted, manifest.chunked, manifest.chunks.len());

    // 2. 下载 payload（分片或整体），逐片校验 SHA256
    let remote_prefix = format!(
        "{}/v1/{}/{}",
        normalize_root(&webdav_cfg.remote_root), req.save_name, req.backup_id
    );
    let mut payload = Vec::new();
    if manifest.chunked {
        let total_chunks = manifest.chunks.len() as u64;
        let dl_start = Instant::now();
        let mut dl_bytes = 0u64;
        for (i, chunk) in manifest.chunks.iter().enumerate() {
            should_cancel(&flag)?;
            let bytes = client.get_bytes(&format!("{remote_prefix}/chunks/{}", chunk.name)).await?;
            if sha256_hex(&bytes) != chunk.sha256 {
                return Err(anyhow!("Chunk checksum mismatch: {}", chunk.name));
            }
            dl_bytes += bytes.len() as u64;
            payload.extend_from_slice(&bytes);
            let percent = 15 + (((i + 1) as f64 / total_chunks as f64) * 45.0) as u8;
            let elapsed = dl_start.elapsed().as_secs_f64().max(0.001);
            let speed = (dl_bytes as f64 / elapsed) as u64;
            emit_progress(&app, &task_id, "download", percent.min(60), dl_bytes, manifest.compressed_size, "Downloading chunks", speed);
        }
    } else {
        let app2 = app.clone();
        let tid2 = task_id.clone();
        let dl_start = Instant::now();
        payload = client.get_bytes_with_progress(
            &format!("{remote_prefix}/payload.bin"),
            move |done, total| {
                let percent = if total > 0 {
                    15 + ((done as f64 / total as f64) * 45.0) as u8
                } else { 30 };
                let elapsed = dl_start.elapsed().as_secs_f64().max(0.001);
                let speed = (done as f64 / elapsed) as u64;
                emit_progress(&app2, &tid2, "download", percent.min(60), done, total, "Downloading payload", speed);
            },
        ).await?;
    }

    // 3. 校验整体 payload SHA256
    emit_progress(&app, &task_id, "verify_download", 62, 0, 0, "Verifying download checksum", 0);
    if sha256_hex(&payload) != manifest.payload_sha256 {
        emit_progress(&app, &task_id, "verify_download_fail", 62, 0, 0, "Download checksum mismatch", 0);
        return Err(anyhow!("Payload checksum mismatch"));
    }
    emit_progress(&app, &task_id, "verify_download_ok", 63, 0, 0, "Download checksum verified", 0);

    // 4. 解密（若加密）— 兼容旧整体加密和新分段加密
    if manifest.encrypted {
        let password = req.encryption_password.as_deref()
            .ok_or_else(|| anyhow!("Encryption password is required"))?;
        let meta = manifest.encryption_meta.as_ref()
            .ok_or_else(|| anyhow!("Missing encryption metadata"))?;
        emit_progress(&app, &task_id, "decrypt", 70, 0, 0, "Decrypting payload", 0);
        let decrypted = if meta.stream_encryption.unwrap_or(false) {
            decrypt_aes_gcm_stream(password, &payload, meta)?
        } else {
            decrypt_aes_gcm(password, &payload, meta)?
        };
        drop(payload);
        payload = decrypted;
    } else {
        // 跳过解密，不发送 phase
    }

    // 5. 流式 zstd 解压 + 增量 SHA256 校验（若未压缩则直接校验）
    let restored;
    if manifest.compressed {
        emit_progress(&app, &task_id, "decompress", 80, 0, 0, "Decompressing payload", 0);
        let mut decoder = zstd::Decoder::new(std::io::Cursor::new(&payload))?;
        let mut buf_out = Vec::with_capacity(manifest.original_size as usize);
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = decoder.read(&mut buf)?;
            if n == 0 { break; }
            buf_out.extend_from_slice(&buf[..n]);
        }
        drop(payload);
        restored = buf_out;
        let restored_sha = hex::encode(sha2::Sha256::digest(&restored));
        info!("[restore:{}] decompressed to {} bytes", task_id, restored.len());
        emit_progress(&app, &task_id, "verify_decompress", 88, 0, 0, "Verifying decompressed checksum", 0);
        if restored_sha != manifest.original_sha256 {
            emit_progress(&app, &task_id, "verify_decompress_fail", 88, 0, 0, "Decompressed checksum mismatch", 0);
            return Err(anyhow!("Restored file checksum mismatch"));
        }
        emit_progress(&app, &task_id, "verify_decompress_ok", 90, 0, 0, "Decompressed checksum verified", 0);
    } else {
        // 跳过解压，不发送 phase
        restored = payload;
    }

    // 6. 写入目标文件，按冲突策略处理同名文件
    let rel = sanitize_relative_path(&manifest.source_relative_path)?;
    let target_root = PathBuf::from(&req.target_dir);
    fs::create_dir_all(&target_root)?;
    let desired_path = target_root.join(&rel);
    let final_path = if desired_path.exists() {
        match req.conflict_policy {
            ConflictPolicy::Overwrite => desired_path,
            ConflictPolicy::Rename => rename_path(&desired_path, &target_root),
            ConflictPolicy::Ask => {
                // 创建 oneshot channel，emit 事件等待前端响应
                let (tx, rx) = tokio::sync::oneshot::channel::<String>();
                if let Ok(mut map) = state.conflict_channels.lock() {
                    map.insert(task_id.clone(), tx);
                }
                let _ = app.emit("conflict_found", ConflictFound {
                    task_id: task_id.clone(),
                    file_path: desired_path.to_string_lossy().to_string(),
                });
                let action = rx.await.map_err(|_| anyhow!("Conflict channel closed"))?;
                match action.as_str() {
                    "overwrite" => desired_path,
                    "rename" => rename_path(&desired_path, &target_root),
                    _ => return Err(anyhow!("Restore cancelled by user")),
                }
            }
        }
    } else {
        desired_path
    };
    if let Some(parent) = final_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&final_path, &restored)?;
    info!("[restore:{}] wrote {} bytes to {}", task_id, restored.len(), final_path.display());
    emit_progress(&app, &task_id, "done", 100, 0, 0, &format!("Restored to {}", final_path.display()), 0);
    Ok(())
}
