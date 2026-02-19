<script setup lang="ts">
import { ref, computed } from "vue";
import { NCard, NButton, NTag, NEmpty, NScrollbar, NList, NListItem, NCheckbox, NModal, NProgress, NInput } from "naive-ui";
import type { ConflictFound, RemoteBackup, TaskProgress } from "../types";
import { formatSize, formatSpeed } from "../utils/format";

const props = defineProps<{
  backups: RemoteBackup[];
  restoreTargetDir: string;
  saveRoot: string;
  encryptionPassword: string;
  taskStatus: Record<string, TaskProgress & { done?: boolean; failed?: boolean; cancelled?: boolean; error?: string }>;
  lastTaskId: string;
  conflictState: ConflictFound | null;
  onDelete: (saveName: string, backupId: string) => Promise<void>;
  onSelectDir: () => void;
  onResolveConflict: (taskId: string, action: string) => void;
}>();

const emit = defineEmits<{
  refresh: [];
  restore: [backup: RemoteBackup, targetDir: string, password: string | null];
  reload: [];
  cancel: [taskId: string];
}>();

const selected = ref<Set<string>>(new Set());
const showDeleteConfirm = ref(false);
const deleting = ref(false);

// 前置检查弹窗状态
const pendingBackup = ref<RemoteBackup | null>(null);
const showDirDialog = ref(false);
const showPwDialog = ref(false);
const inlinePw = ref("");
const resolvedDir = ref("");

// ── 恢复前置检查流程 ──
function requestRestore(b: RemoteBackup) {
  pendingBackup.value = b;
  resolvedDir.value = "";
  inlinePw.value = "";
  // 步骤1：检查恢复目录
  if (!props.restoreTargetDir) {
    showDirDialog.value = true;
  } else {
    resolvedDir.value = props.restoreTargetDir;
    checkPassword();
  }
}

function onDirUseSaveRoot() {
  resolvedDir.value = props.saveRoot;
  showDirDialog.value = false;
  checkPassword();
}

function onDirSelectCustom() {
  showDirDialog.value = false;
  props.onSelectDir();
  // onSelectDir 是异步的，用 watch 等待 restoreTargetDir 变化不太方便
  // 简单做法：设一个轮询等待
  const check = setInterval(() => {
    if (props.restoreTargetDir) {
      clearInterval(check);
      resolvedDir.value = props.restoreTargetDir;
      checkPassword();
    }
  }, 200);
  // 10秒超时取消
  setTimeout(() => clearInterval(check), 10000);
}

function onDirCancel() {
  showDirDialog.value = false;
  pendingBackup.value = null;
}

// 步骤2：检查密码
function checkPassword() {
  const b = pendingBackup.value;
  if (!b) return;
  if (!b.encrypted) {
    doRestore(null);
  } else if (props.encryptionPassword) {
    // 有加密密码可用，但仍需询问
    showPwDialog.value = true;
  } else {
    showPwDialog.value = true;
  }
}

function onPwUseEncrypt() {
  showPwDialog.value = false;
  doRestore(props.encryptionPassword);
}

function onPwUseInline() {
  showPwDialog.value = false;
  doRestore(inlinePw.value);
}

function onPwCancel() {
  showPwDialog.value = false;
  pendingBackup.value = null;
}

function doRestore(pw: string | null) {
  const b = pendingBackup.value;
  if (!b || !resolvedDir.value) return;
  emit("restore", b, resolvedDir.value, pw);
  pendingBackup.value = null;
}

// ── 批量恢复 ──
function batchRestore() {
  for (const b of selectedBackups.value) requestRestore(b);
}

// ── 进度条 ──
const RESTORE_PHASES = [
  { key: "manifest",   label: "下载清单",  icon: "fas fa-file-code" },
  { key: "download",   label: "下载数据",  icon: "fas fa-cloud-download-alt" },
  { key: "decrypt",    label: "解密",      icon: "fas fa-unlock-alt" },
  { key: "decompress", label: "解压",      icon: "fas fa-expand-alt" },
  { key: "done",       label: "完成",      icon: "fas fa-check-circle" },
];
const PHASE_ORDER = RESTORE_PHASES.map(p => p.key);

const task = computed(() => props.lastTaskId ? props.taskStatus[props.lastTaskId] ?? null : null);
const isRestoreTask = computed(() => props.lastTaskId.startsWith("restore_"));

type PhaseState = "done" | "active" | "stopped" | "pending";
function phaseState(key: string): PhaseState {
  if (!task.value) return "pending";
  const cur = task.value.phase;
  if (cur === "done") return "done";
  const curIdx = PHASE_ORDER.indexOf(cur);
  const keyIdx = PHASE_ORDER.indexOf(key);
  if (cur === "cancelled" || cur === "failed") {
    if (keyIdx < curIdx) return "done";
    if (keyIdx === curIdx) return "stopped";
    return "pending";
  }
  if (keyIdx < curIdx) return "done";
  if (keyIdx === curIdx) return "active";
  return "pending";
}

const progressStatus = computed(() => {
  if (!task.value) return "default" as const;
  if (task.value.phase === "done") return "success" as const;
  if (task.value.phase === "failed") return "error" as const;
  if (task.value.phase === "cancelled") return "warning" as const;
  return "default" as const;
});

const isTerminal = computed(() =>
  task.value ? ["done", "failed", "cancelled"].includes(task.value.phase) : false
);

const downloadDetail = computed(() => {
  if (!task.value || task.value.phase !== "download") return null;
  const { bytesDone, bytesTotal, speedBps } = task.value;
  if (bytesTotal <= 0) return null;
  const speed = speedBps > 0 ? ` · ${formatSpeed(speedBps)}` : "";
  return `${formatSize(bytesDone)} / ${formatSize(bytesTotal)}${speed}`;
});

type VerifyState = "none" | "checking" | "ok" | "fail";
function verifyState(target: "download" | "decompress"): VerifyState {
  if (!task.value) return "none";
  const p = task.value.phase;
  if (p === `verify_${target}`) return "checking";
  if (p === `verify_${target}_ok`) return "ok";
  if (p === `verify_${target}_fail`) return "fail";
  const order = ["download", "verify_download", "verify_download_ok", "decrypt", "decompress", "verify_decompress", "verify_decompress_ok", "done"];
  const curIdx = order.indexOf(p);
  const okIdx = order.indexOf(`verify_${target}_ok`);
  if (curIdx > okIdx) return "ok";
  return "none";
}

// ── 通用工具 ──
function backupKey(b: RemoteBackup) {
  return `${b.saveName}::${b.backupId}`;
}

function toggleSelect(b: RemoteBackup) {
  const k = backupKey(b);
  const s = new Set(selected.value);
  s.has(k) ? s.delete(k) : s.add(k);
  selected.value = s;
}

const allSelected = computed(() =>
  props.backups.length > 0 && props.backups.every(b => selected.value.has(backupKey(b)))
);

function toggleAll() {
  if (allSelected.value) {
    selected.value = new Set();
  } else {
    selected.value = new Set(props.backups.map(backupKey));
  }
}

const selectedBackups = computed(() =>
  props.backups.filter(b => selected.value.has(backupKey(b)))
);

async function confirmDelete() {
  deleting.value = true;
  for (const b of selectedBackups.value) {
    await props.onDelete(b.saveName, b.backupId);
  }
  deleting.value = false;
  showDeleteConfirm.value = false;
  selected.value = new Set();
  emit("reload");
}

function chunkCount(b: RemoteBackup) {
  if (!b.chunked || b.compressedSize <= 0) return 0;
  return Math.ceil(b.compressedSize / (10 * 1024 * 1024));
}
</script>

<template>
  <div>
  <n-card>
    <template #header>
      <span><i class="fas fa-cloud-download-alt" style="margin-right:6px"></i>从云端恢复存档</span>
    </template>
    <template #header-extra>
      <n-button text size="small" @click="emit('refresh')">
        <i class="fas fa-rotate-right" style="margin-right:4px"></i>刷新列表
      </n-button>
    </template>

    <div class="restore-opts">
      <n-button @click="props.onSelectDir()">
        <i class="fas fa-folder-open" style="margin-right:4px"></i>选择恢复目录
      </n-button>
      <span v-if="restoreTargetDir" class="selected-dir">{{ restoreTargetDir }}</span>
      <span v-else class="selected-dir placeholder">未选择目录</span>
    </div>

    <n-empty v-if="backups.length === 0" description="暂无云端备份" style="margin-top:16px" />
    <template v-else>
      <n-scrollbar style="max-height:calc(100vh - 340px)">
        <n-list hoverable>
          <n-list-item v-for="b in backups" :key="backupKey(b)">
            <template #prefix>
              <div class="prefix-wrap">
                <n-checkbox :checked="selected.has(backupKey(b))" @update:checked="toggleSelect(b)" />
                <i class="fas fa-file-zipper cloud-icon"></i>
              </div>
            </template>

            <div class="cloud-title">
              {{ b.sourceRelativePath || b.saveName }}
              <n-tag v-if="b.encrypted" size="tiny" round type="warning" class="title-tag">加密</n-tag>
              <n-tag v-if="b.chunked" size="tiny" round type="info" class="title-tag">分片</n-tag>
            </div>
            <div class="cloud-details">
              <span><i class="far fa-calendar"></i> {{ b.createdAt.slice(0, 16).replace("T", " ") }}</span>
              <span><i class="fas fa-weight-hanging"></i> {{ formatSize(b.compressedSize) }}</span>
              <span v-if="b.chunked"><i class="fas fa-cut"></i> {{ chunkCount(b) }} 片</span>
              <span v-if="b.encrypted" class="detail-encrypted"><i class="fas fa-lock"></i> 需要密码</span>
              <span v-else class="detail-plain"><i class="fas fa-unlock-alt"></i> 无加密</span>
            </div>
            <div class="cloud-id">{{ b.backupId }}</div>

            <template #suffix>
              <n-button size="small" round type="primary" @click="requestRestore(b)">
                <i class="fas fa-download" style="margin-right:4px"></i>恢复
              </n-button>
            </template>
          </n-list-item>
        </n-list>
      </n-scrollbar>

      <!-- 批量操作栏 -->
      <div class="batch-bar">
        <n-checkbox :checked="allSelected" @update:checked="toggleAll">全选</n-checkbox>
        <span class="batch-info">已选 {{ selected.size }} 项</span>
        <n-button size="small" :disabled="selected.size === 0" @click="batchRestore">
          <i class="fas fa-download" style="margin-right:4px"></i>批量恢复
        </n-button>
        <n-button size="small" type="error" :disabled="selected.size === 0" @click="showDeleteConfirm = true">
          <i class="fas fa-trash-alt" style="margin-right:4px"></i>批量删除
        </n-button>
      </div>
    </template>
  </n-card>

  <!-- 恢复任务进度 -->
  <n-card v-if="task && isRestoreTask" style="margin-top:16px">
    <template #header>
      <span><i class="fas fa-tasks" style="margin-right:6px"></i>恢复进度</span>
      <n-tag :type="task.phase === 'done' ? 'success' : task.phase === 'cancelled' ? 'warning' : task.phase === 'failed' ? 'error' : 'info'"
        round size="small" style="margin-left:10px">
        {{ task.phase === 'done' ? '完成' : task.phase === 'cancelled' ? '已终止' : task.phase === 'failed' ? '失败' : '进行中' }}
      </n-tag>
    </template>
    <template #header-extra>
      <n-button v-if="!isTerminal" size="small" round type="error" ghost @click="emit('cancel', lastTaskId)">
        <i class="fas fa-stop" style="margin-right:4px"></i>停止
      </n-button>
    </template>

    <div class="progress-header">
      <span class="progress-label">{{ task.message || task.phase }}</span>
      <span class="progress-pct">{{ task.percent }}%</span>
    </div>
    <n-progress type="line" :percentage="task.percent" :status="progressStatus" :show-indicator="false" style="margin-bottom:16px" />

    <div class="phase-steps">
      <div v-for="p in RESTORE_PHASES" :key="p.key" class="phase-step" :class="phaseState(p.key)">
        <div class="phase-icon">
          <i v-if="phaseState(p.key) === 'done'" class="fas fa-check-circle" style="color:#48b58b"></i>
          <i v-else-if="phaseState(p.key) === 'active'" class="spinner-icon fas fa-spinner" style="color:#6ea8fe"></i>
          <i v-else-if="phaseState(p.key) === 'stopped'" class="fas fa-times-circle" style="color:#e57373"></i>
          <i v-else :class="p.icon" style="color:#4a566b"></i>
        </div>
        <div class="phase-info">
          <span class="phase-label">
            {{ p.label }}
            <n-tag v-if="p.key === 'download' && verifyState('download') === 'checking'" size="tiny" round type="info" class="verify-tag">
              <i class="fas fa-spinner spinner-icon"></i> 校验中
            </n-tag>
            <n-tag v-else-if="p.key === 'download' && verifyState('download') === 'ok'" size="tiny" round type="success" class="verify-tag">
              <i class="fas fa-check"></i> 校验通过
            </n-tag>
            <n-tag v-else-if="p.key === 'download' && verifyState('download') === 'fail'" size="tiny" round type="error" class="verify-tag">
              <i class="fas fa-times"></i> 校验失败
            </n-tag>
            <n-tag v-if="p.key === 'decompress' && verifyState('decompress') === 'checking'" size="tiny" round type="info" class="verify-tag">
              <i class="fas fa-spinner spinner-icon"></i> 校验中
            </n-tag>
            <n-tag v-else-if="p.key === 'decompress' && verifyState('decompress') === 'ok'" size="tiny" round type="success" class="verify-tag">
              <i class="fas fa-check"></i> 校验通过
            </n-tag>
            <n-tag v-else-if="p.key === 'decompress' && verifyState('decompress') === 'fail'" size="tiny" round type="error" class="verify-tag">
              <i class="fas fa-times"></i> 校验失败
            </n-tag>
          </span>
          <span v-if="p.key === 'download' && phaseState('download') === 'active' && downloadDetail"
            class="phase-detail">{{ downloadDetail }}</span>
        </div>
      </div>
    </div>

    <div v-if="task.failed && task.error" class="error-msg">
      <i class="fas fa-exclamation-circle"></i> {{ task.error }}
    </div>
  </n-card>

  <!-- 删除确认弹窗 -->
  <n-modal v-model:show="showDeleteConfirm" preset="card" title="确认删除" style="width:400px" :mask-closable="!deleting">
    <p>确定要删除以下 <b>{{ selected.size }}</b> 个云端备份吗？此操作不可撤销。</p>
    <ul class="del-list">
      <li v-for="b in selectedBackups" :key="backupKey(b)">
        {{ b.sourceRelativePath || b.saveName }} — {{ b.backupId }}
      </li>
    </ul>
    <div class="del-actions">
      <n-button :disabled="deleting" @click="showDeleteConfirm = false">取消</n-button>
      <n-button type="error" :loading="deleting" @click="confirmDelete">
        <i class="fas fa-trash-alt" style="margin-right:4px"></i>确认删除
      </n-button>
    </div>
  </n-modal>

  <!-- 冲突询问弹窗 -->
  <n-modal :show="!!conflictState" preset="card" title="文件冲突" style="width:440px" :mask-closable="false">
    <template v-if="conflictState">
      <p>目标位置已存在同名文件：</p>
      <div class="conflict-path">{{ conflictState.filePath }}</div>
      <p>请选择处理方式：</p>
      <div class="del-actions">
        <n-button @click="props.onResolveConflict(conflictState!.taskId, 'cancel')">取消恢复</n-button>
        <n-button @click="props.onResolveConflict(conflictState!.taskId, 'rename')">重命名</n-button>
        <n-button type="warning" @click="props.onResolveConflict(conflictState!.taskId, 'overwrite')">覆盖</n-button>
      </div>
    </template>
  </n-modal>

  <!-- 恢复目录选择弹窗 -->
  <n-modal :show="showDirDialog" @update:show="(v: boolean) => { if (!v) onDirCancel() }" preset="card" title="选择恢复目录" style="width:440px">
    <p>尚未设置恢复目录，请选择：</p>
    <div v-if="saveRoot" class="conflict-path">备份目录：{{ saveRoot }}</div>
    <div class="del-actions">
      <n-button :disabled="!saveRoot" @click="onDirUseSaveRoot">使用备份目录</n-button>
      <n-button type="warning" @click="onDirSelectCustom">自选目录</n-button>
      <n-button type="error" ghost @click="onDirCancel">取消</n-button>
    </div>
  </n-modal>

  <!-- 解密密码弹窗 -->
  <n-modal :show="showPwDialog" @update:show="(v: boolean) => { if (!v) onPwCancel() }" preset="card" title="输入解密密码" style="width:440px">
    <p>该备份已加密，请提供解密密码：</p>
    <n-input v-model:value="inlinePw" type="password" show-password-on="click" placeholder="输入解密密码" style="margin-bottom:12px" />
    <div class="del-actions">
      <n-button v-if="encryptionPassword" @click="onPwUseEncrypt">使用加密密码</n-button>
      <n-button type="primary" :disabled="!inlinePw" @click="onPwUseInline">确认</n-button>
      <n-button type="error" ghost @click="onPwCancel">取消</n-button>
    </div>
  </n-modal>
  </div>
</template>

<style scoped>
.restore-opts { display: flex; gap: 10px; margin-bottom: 16px; flex-wrap: wrap; align-items: center; }
.selected-dir { font-size: 13px; color: var(--text-sub); word-break: break-all; }
.selected-dir.placeholder { color: var(--text-muted); font-style: italic; }
.conflict-path {
  font-family: monospace; font-size: 12px; color: var(--text);
  background: var(--bg-item); padding: 8px 12px; border-radius: 6px;
  word-break: break-all; margin: 8px 0;
}

.cloud-icon { font-size: 20px; color: var(--accent); }
.prefix-wrap { display: flex; align-items: center; gap: 8px; }

.cloud-title {
  color: var(--text); font-weight: 500; font-size: 14px;
  display: flex; align-items: center; gap: 6px;
}
.title-tag { margin-left: 2px; }

.cloud-details {
  color: var(--text-sub); font-size: 12px; margin-top: 5px;
  display: flex; align-items: center; gap: 18px; flex-wrap: wrap;
}
.cloud-details i { margin-right: 4px; font-size: 11px; }
.detail-encrypted { color: #f0a020; }
.detail-plain { color: var(--text-muted); }

.cloud-id {
  color: var(--text-muted); font-size: 11px; margin-top: 3px;
  font-family: monospace;
}

.batch-bar {
  display: flex; align-items: center; gap: 12px;
  margin-top: 12px; padding: 10px 12px;
  background: var(--bg-item); border-radius: 8px;
}
.batch-info { color: var(--text-sub); font-size: 12px; margin-right: auto; }

.del-list {
  margin: 10px 0; padding-left: 20px;
  font-size: 12px; color: var(--text-sub);
  max-height: 160px; overflow-y: auto;
}
.del-actions { display: flex; justify-content: flex-end; gap: 10px; margin-top: 16px; }

/* 进度条 */
.progress-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 6px; }
.progress-label { font-size: 13px; color: var(--text-sub); }
.progress-pct { font-size: 14px; font-weight: 600; color: var(--text); }
.phase-steps { display: flex; flex-direction: column; gap: 6px; }
.phase-step {
  display: flex; align-items: center; gap: 10px;
  padding: 7px 10px; border-radius: 8px;
  background: var(--bg-item); opacity: 0.45;
  transition: opacity 0.2s, background 0.2s;
}
.phase-step.done { opacity: 0.75; }
.phase-step.active { opacity: 1; background: var(--bg-active, rgba(110,168,254,0.08)); }
.phase-step.stopped { opacity: 1; background: rgba(229,115,115,0.08); }
.phase-icon { width: 18px; text-align: center; font-size: 14px; flex-shrink: 0; }
.phase-info { flex: 1; display: flex; flex-direction: column; gap: 2px; }
.phase-label { font-size: 13px; color: var(--text); display: flex; align-items: center; gap: 6px; }
.verify-tag i { margin-right: 3px; font-size: 10px; }
.phase-detail { font-size: 11px; color: var(--text-muted); }
.error-msg { margin-top: 10px; font-size: 12px; color: #e57373; }
</style>
