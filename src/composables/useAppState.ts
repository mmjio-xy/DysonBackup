import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { ConflictFound, LocalSaveFile, RemoteBackup, TaskDone, TaskProgress } from "../types";

export function useAppState() {
  const saveRoot = ref("");
  const detectedPaths = ref<string[]>([]);
  const files = ref<LocalSaveFile[]>([]);
  const backups = ref<RemoteBackup[]>([]);
  const logs = ref<string[]>([]);
  const taskStatus = ref<Record<string, TaskProgress & { done?: boolean; failed?: boolean; cancelled?: boolean; error?: string }>>({});
  const lastTaskId = ref("");
  const cancelledTasks = new Set<string>();

  // WebDAV settings
  const baseUrl = ref("");
  const username = ref("");
  const webdavPassword = ref("");
  const webdavPasswordSet = ref(false);
  const remoteRoot = ref("dysonbackup");

  // Encryption settings
  const encryptByDefault = ref(false);
  const encryptionPassword = ref("");

  // Restore settings
  const restoreTargetDir = ref("");
  const decryptionPassword = ref("");
  const useEncryptPwForRestore = ref(false);

  // Conflict dialog state
  const conflictState = ref<ConflictFound | null>(null);

  // Debug
  const debugMode = ref(false);

  // Tray
  const closeAction = ref("ask");

  // Compress
  const compressEnabled = ref(true);
  const compressLevel = ref(6);

  function addLog(line: string) {
    const t = new Date().toTimeString().slice(0, 8);
    logs.value = [`${t} ${line}`, ...logs.value].slice(0, 200);
  }

  async function detectPaths() {
    const r = await invoke<{ candidates: string[] }>("detect_save_paths");
    detectedPaths.value = r.candidates;
    if (!saveRoot.value && r.candidates.length > 0) saveRoot.value = r.candidates[0];
  }

  async function savePath() {
    await invoke("set_save_root", { path: saveRoot.value });
    addLog(`存档目录已设置: ${saveRoot.value}`);
  }

  async function scanSaves() {
    files.value = await invoke<LocalSaveFile[]>("scan_saves");
    addLog(`扫描完成，共 ${files.value.length} 个文件`);
  }

  async function saveWebDavConfig() {
    await invoke("save_webdav_config", {
      input: { baseUrl: baseUrl.value, username: username.value, password: webdavPassword.value, remoteRoot: remoteRoot.value },
    });
    addLog("WebDAV 配置已保存");
  }

  async function testWebDav() {
    return await invoke<{
      serverReachable: boolean; serverMessage: string;
      authOk: boolean; authMessage: string;
      remoteDirExists: boolean; remoteDirMessage: string;
      overallOk: boolean;
    }>("test_webdav_connection");
  }

  async function startBackup(file: LocalSaveFile) {
    const taskId = await invoke<string>("start_backup", {
      req: {
        localFilePath: file.localFilePath,
        saveName: file.saveName,
        sourceRelativePath: file.relativePath,
        useEncryption: encryptByDefault.value,
        encryptionPassword: encryptByDefault.value ? encryptionPassword.value : null,
        useCompression: compressEnabled.value,
        compressionLevel: compressLevel.value,
      },
    });
    taskStatus.value[taskId] = { taskId, phase: "read", percent: 0, bytesDone: 0, bytesTotal: 0, message: "", speedBps: 0 };
    lastTaskId.value = taskId;
    addLog(`开始备份: ${file.relativePath}`);
    return taskId;
  }

  async function cancelTask(taskId: string) {
    cancelledTasks.add(taskId);
    await invoke("cancel_task", { taskId });
  }

  async function loadBackups() {
    try {
      backups.value = await invoke<RemoteBackup[]>("list_remote_backups");
      addLog(`云端备份列表已刷新，共 ${backups.value.length} 条`);
    } catch {
      // WebDAV 未配置时静默跳过
    }
  }

  async function deleteBackup(saveName: string, backupId: string) {
    await invoke("delete_remote_backup", { saveName, backupId });
    addLog(`已删除备份: ${saveName}/${backupId}`);
  }

  async function setDebugMode(enabled: boolean) {
    await invoke("set_debug_mode", { enabled });
    debugMode.value = enabled;
  }

  async function setCloseAction(action: string) {
    await invoke("set_close_action", { action });
    closeAction.value = action;
  }

  async function setCompressConfig(enabled: boolean, level: number) {
    await invoke("set_compress_config", { enabled, level });
    compressEnabled.value = enabled;
    compressLevel.value = level;
  }

  async function saveEncryptionSettings() {
    await invoke("save_encryption_settings", {
      encryptByDefault: encryptByDefault.value,
      password: encryptionPassword.value || null,
    });
    addLog("加密设置已保存");
  }

  async function restore(backup: RemoteBackup, targetDir: string, password: string | null) {
    try {
      const taskId = await invoke<string>("start_restore", {
        req: {
          saveName: backup.saveName,
          backupId: backup.backupId,
          targetDir,
          conflictPolicy: "ask",
          encryptionPassword: password,
        },
      });
      taskStatus.value[taskId] = { taskId, phase: "manifest", percent: 0, bytesDone: 0, bytesTotal: 0, message: "", speedBps: 0 };
      lastTaskId.value = taskId;
      addLog(`开始恢复: ${backup.backupId}`);
      return taskId;
    } catch (e) {
      addLog(`恢复失败: ${e}`);
    }
  }

  async function selectRestoreDir() {
    const dir = await open({ directory: true, multiple: false, title: "选择恢复目录" });
    if (dir) restoreTargetDir.value = dir as string;
  }

  async function resolveConflict(taskId: string, action: string) {
    await invoke("resolve_conflict", { taskId, action });
    conflictState.value = null;
  }

  const lastTask = computed(() => lastTaskId.value ? taskStatus.value[lastTaskId.value] ?? null : null);
  const totalLocalSize = computed(() => files.value.reduce((a, v) => a + v.size, 0));
  const totalCloudSize = computed(() => backups.value.reduce((a, v) => a + v.compressedSize, 0));
  const encryptedCount = computed(() => backups.value.filter((b) => b.encrypted).length);
  const chunkedCount = computed(() => backups.value.filter((b) => b.chunked).length);
  const latestBackup = computed(() => backups.value[0] ?? null);
  const runningTasks = computed(() =>
    Object.entries(taskStatus.value).filter(([, v]) => v.phase !== "done" && v.phase !== "failed")
  );

  onMounted(async () => {
    // 先读取已保存的配置
    const cfg = await invoke<{
      saveRoot?: string;
      webdav?: { baseUrl: string; username: string; remoteRoot: string };
      webdavPasswordSet?: boolean;
      debugMode?: boolean;
      encryptByDefault?: boolean;
      encryptionPasswordSet?: boolean;
      closeAction?: string;
      compressEnabled?: boolean;
      compressLevel?: number;
    }>("get_config");
    if (cfg.saveRoot) saveRoot.value = cfg.saveRoot;
    if (cfg.webdav) {
      baseUrl.value = cfg.webdav.baseUrl;
      username.value = cfg.webdav.username;
      remoteRoot.value = cfg.webdav.remoteRoot;
    }
    webdavPasswordSet.value = cfg.webdavPasswordSet ?? false;
    debugMode.value = cfg.debugMode ?? false;
    encryptByDefault.value = cfg.encryptByDefault ?? false;
    closeAction.value = cfg.closeAction ?? "ask";
    compressEnabled.value = cfg.compressEnabled ?? true;
    compressLevel.value = cfg.compressLevel ?? 6;
    if (cfg.encryptionPasswordSet) {
      try {
        encryptionPassword.value = await invoke<string>("get_encryption_password");
      } catch { /* keyring 读取失败则忽略 */ }
    }

    await detectPaths();
    await listen<TaskProgress>("task_progress", (e) => {
      const p = e.payload;
      taskStatus.value[p.taskId] = { ...p };
    });
    await listen<TaskDone>("task_done", (e) => {
      const d = e.payload;
      const prev = taskStatus.value[d.taskId] ?? { taskId: d.taskId, bytesDone: 0, bytesTotal: 0, message: "" };
      const isCancelled = cancelledTasks.has(d.taskId);
      const phase = d.success ? "done" : isCancelled ? "cancelled" : "failed";
      taskStatus.value[d.taskId] = { ...prev, phase, percent: d.success ? 100 : prev.percent, done: d.success, failed: !d.success && !isCancelled, cancelled: isCancelled, error: d.error };
      if (isCancelled) cancelledTasks.delete(d.taskId);
      addLog(`${d.taskId} ${d.success ? "完成" : isCancelled ? "已终止" : "失败: " + d.error}`);
    });
    await listen<ConflictFound>("conflict_found", (e) => {
      conflictState.value = e.payload;
    });
    // // 文件变更防抖刷新（暂时禁用）
    // let debounceTimer: ReturnType<typeof setTimeout> | null = null;
    // await listen<FileChanged>("file_changed", (e) => {
    //   addLog(`文件变更: ${e.payload.kind} ${e.payload.path}`);
    //   if (debounceTimer) clearTimeout(debounceTimer);
    //   debounceTimer = setTimeout(() => scanSaves(), 2000);
    // });
    await scanSaves();
    await loadBackups();
  });

  return {
    saveRoot, detectedPaths, files, backups, logs, taskStatus, lastTaskId, lastTask,
    baseUrl, username, webdavPassword, webdavPasswordSet, remoteRoot,
    encryptByDefault, encryptionPassword,
    restoreTargetDir, decryptionPassword, useEncryptPwForRestore, conflictState,
    totalLocalSize, totalCloudSize, encryptedCount, chunkedCount, latestBackup, runningTasks,
    debugMode, setDebugMode, saveEncryptionSettings,
    closeAction, setCloseAction,
    compressEnabled, compressLevel, setCompressConfig,
    detectPaths, savePath, scanSaves, saveWebDavConfig, testWebDav, startBackup, loadBackups,
    restore, cancelTask, deleteBackup, selectRestoreDir, resolveConflict,
  };
}
