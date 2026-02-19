# DysonBackup

戴森球计划存档云备份工具，基于 Tauri v2 + Vue 3 + Naive UI 构建。

## 功能

- 自动探测戴森球计划存档目录
- 存档文件扫描与 SHA256 校验
- WebDAV 云端备份（支持分片上传、流式进度显示）
- AES-256-GCM 加密（可选，分段流式加密）
- zstd 流式压缩
- 云端恢复（下载 → 校验 → 解密 → 解压）
- 实时传输速度显示
- 任务取消支持
- 文件冲突处理（覆盖 / 重命名 / 询问）
- 深色主题

## 技术栈

| 层 | 技术 |
|---|------|
| 前端 | Vue 3 + TypeScript + Naive UI |
| 后端 | Rust + Tauri v2 |
| HTTP | reqwest（流式上传/下载） |
| 加密 | AES-256-GCM + PBKDF2 |
| 压缩 | zstd |
| 密码存储 | Windows Credential Manager (keyring) |

## 开发

```bash
# 安装依赖
npm install

# 开发模式
npm run tauri dev

# 构建
npm run tauri build
```

## 远端存储结构

```
{remote_root}/v1/{save_name}/{backup_id}/
  manifest.json                    # 备份元数据
  payload.bin                      # 单文件（< 100MB）
  chunks/chunk_XXXXXX.part         # 分片（>= 100MB）
```

## 许可证

[GPL-3.0](LICENSE)
