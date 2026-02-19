<script setup lang="ts">
import { computed } from "vue";
import { NCard, NButton, NProgress, NTag, NScrollbar, NEmpty, NList, NListItem } from "naive-ui";
import type { LocalSaveFile, TaskProgress } from "../types";
import { formatSize, formatTime, formatSpeed } from "../utils/format";

const props = defineProps<{
  files: LocalSaveFile[];
  logs: string[];
  taskStatus: Record<string, TaskProgress & { done?: boolean; failed?: boolean; cancelled?: boolean; error?: string }>;
  lastTaskId: string;
}>();

const emit = defineEmits<{
  scan: [];
  backup: [file: LocalSaveFile];
  cancel: [taskId: string];
}>();

const BACKUP_PHASES = [
  { key: "read",     label: "读取文件",  icon: "fas fa-file-alt" },
  { key: "compress", label: "zstd 压缩", icon: "fas fa-compress-alt" },
  { key: "encrypt",  label: "AES 加密",  icon: "fas fa-lock" },
  { key: "upload",   label: "上传",      icon: "fas fa-cloud-upload-alt" },
  { key: "manifest", label: "写清单",    icon: "fas fa-file-code" },
  { key: "done",     label: "完成",      icon: "fas fa-check-circle" },
];
const PHASE_ORDER = BACKUP_PHASES.map(p => p.key);

const task = computed(() => props.lastTaskId ? props.taskStatus[props.lastTaskId] ?? null : null);

type PhaseState = "done" | "active" | "stopped" | "pending";
function phaseState(key: string): PhaseState {
  if (!task.value) return "pending";
  const cur = task.value.phase;
  if (cur === "done") return "done";
  const curIdx = PHASE_ORDER.indexOf(cur);
  const keyIdx = PHASE_ORDER.indexOf(key);
  // cancelled/failed：之前的 done，当前的 stopped，之后的 pending
  if (cur === "cancelled" || cur === "failed") {
    if (keyIdx < curIdx) return "done";
    if (keyIdx === curIdx) return "stopped";
    return "pending";
  }
  if (keyIdx < curIdx) return "done";
  if (keyIdx === curIdx) return "active";
  return "pending";
}

const uploadDetail = computed(() => {
  if (!task.value || task.value.phase !== "upload") return null;
  const { bytesDone, bytesTotal, speedBps } = task.value;
  if (bytesTotal <= 0) return null;
  const speed = speedBps > 0 ? ` · ${formatSpeed(speedBps)}` : "";
  return `${formatSize(bytesDone)} / ${formatSize(bytesTotal)}${speed}`;
});

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
</script>

<template>
  <div class="backup-layout">
    <!-- 左侧：存档列表 -->
    <n-card>
      <template #header>
        <span><i class="far fa-save" style="margin-right:6px"></i>本地存档</span>
      </template>
      <template #header-extra>
        <n-button text size="small" @click="emit('scan')">
          <i class="fas fa-sync-alt" style="margin-right:4px"></i>刷新
        </n-button>
      </template>
      <n-empty v-if="files.length === 0" description="暂无存档" />
      <n-scrollbar v-else style="max-height:calc(100vh - 220px)">
        <n-list hoverable>
          <n-list-item v-for="f in files" :key="f.localFilePath">
            <template #prefix>
              <i class="far fa-file" style="font-size:18px;color:var(--accent)"></i>
            </template>
            <div class="save-name">{{ f.relativePath }}</div>
            <div class="save-meta">{{ formatTime(f.mtimeUnix) }} · {{ formatSize(f.size) }}</div>
            <template #suffix>
              <n-button size="small" round @click="emit('backup', f)">
                <i class="fas fa-cloud-upload-alt" style="margin-right:4px"></i>备份
              </n-button>
            </template>
          </n-list-item>
        </n-list>
      </n-scrollbar>
    </n-card>

    <!-- 右侧 -->
    <div class="right-col">
      <!-- 任务状态卡片 -->
      <n-card>
        <template #header>
          <span><i class="fas fa-tasks" style="margin-right:6px"></i>任务状态</span>
          <n-tag v-if="task" :type="task.phase === 'done' ? 'success' : task.phase === 'cancelled' ? 'warning' : task.phase === 'failed' ? 'error' : 'info'"
            round size="small" style="margin-left:10px">
            {{ task.phase === 'done' ? '完成' : task.phase === 'cancelled' ? '已终止' : task.phase === 'failed' ? '失败' : '进行中' }}
          </n-tag>
        </template>
        <template #header-extra>
          <n-button v-if="task && !isTerminal"
            size="small" round type="error" ghost
            @click="emit('cancel', lastTaskId)">
            <i class="fas fa-stop" style="margin-right:4px"></i>停止
          </n-button>
        </template>

        <template v-if="task">
          <div class="progress-header">
            <span class="progress-label">{{ task.message || task.phase }}</span>
            <span class="progress-pct">{{ task.percent }}%</span>
          </div>
          <n-progress type="line" :percentage="task.percent" :status="progressStatus"
            :show-indicator="false" style="margin-bottom:16px" />

          <div class="phase-steps">
            <div v-for="p in BACKUP_PHASES" :key="p.key"
              class="phase-step" :class="phaseState(p.key)">
              <div class="phase-icon">
                <i v-if="phaseState(p.key) === 'done'" class="fas fa-check-circle" style="color:#48b58b"></i>
                <i v-else-if="phaseState(p.key) === 'active'" class="spinner-icon fas fa-spinner" style="color:#6ea8fe"></i>
                <i v-else-if="phaseState(p.key) === 'stopped'" class="fas fa-times-circle" style="color:#e57373"></i>
                <i v-else :class="p.icon" style="color:#4a566b"></i>
              </div>
              <div class="phase-info">
                <span class="phase-label">{{ p.label }}</span>
                <span v-if="p.key === 'upload' && phaseState('upload') === 'active' && uploadDetail"
                  class="phase-detail">{{ uploadDetail }}</span>
              </div>
              <div v-if="p.key === 'upload' && phaseState('upload') === 'active' && task.bytesTotal > 0"
                class="phase-subbar">
                <div class="phase-subbar-fill"
                  :style="{ width: Math.round(task.bytesDone / task.bytesTotal * 100) + '%' }"></div>
              </div>
            </div>
          </div>

          <div v-if="task.failed && task.error" class="error-msg">
            <i class="fas fa-exclamation-circle"></i> {{ task.error }}
          </div>
        </template>
        <span v-else class="no-task">暂无任务</span>
      </n-card>

      <!-- 操作日志 -->
      <n-card style="flex:1">
        <template #header>
          <span><i class="fas fa-terminal" style="margin-right:6px"></i>操作日志</span>
        </template>
        <n-scrollbar style="max-height:220px">
          <div class="log-panel">
            <div v-for="line in logs" :key="line" class="log-line">{{ line }}</div>
            <div v-if="logs.length === 0" class="log-line muted">暂无日志</div>
          </div>
        </n-scrollbar>
      </n-card>
    </div>
  </div>
</template>

<style scoped>
.backup-layout { display: grid; grid-template-columns: 2fr 1fr; gap: 16px; }
.right-col { display: flex; flex-direction: column; gap: 16px; }

.save-name { color: var(--text); font-weight: 500; font-size: 14px; }
.save-meta { color: var(--text-muted); font-size: 12px; margin-top: 2px; }

/* 进度头部 */
.progress-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 6px; }
.progress-label { font-size: 13px; color: var(--text-sub); }
.progress-pct { font-size: 14px; font-weight: 600; color: var(--text); }

/* 分阶段步骤 */
.phase-steps { display: flex; flex-direction: column; gap: 6px; }
.phase-step {
  display: flex; align-items: center; gap: 10px;
  padding: 7px 10px; border-radius: 8px;
  background: var(--bg-item);
  opacity: 0.45;
  transition: opacity 0.2s, background 0.2s;
}
@keyframes spin { to { transform: rotate(360deg); } }
.phase-step.done { opacity: 0.75; }
.phase-step.active { opacity: 1; background: var(--bg-active, rgba(110,168,254,0.08)); }
.phase-step.stopped { opacity: 1; background: rgba(229,115,115,0.08); }
.phase-icon { width: 18px; text-align: center; font-size: 14px; flex-shrink: 0; }
.phase-info { flex: 1; display: flex; flex-direction: column; gap: 2px; }
.phase-label { font-size: 13px; color: var(--text); }
.phase-detail { font-size: 11px; color: var(--text-muted); }

/* 上传子进度条 */
.phase-subbar {
  width: 60px; height: 4px; background: var(--border); border-radius: 4px; overflow: hidden; flex-shrink: 0;
}
.phase-subbar-fill { height: 100%; background: #6ea8fe; border-radius: 4px; transition: width 0.3s; }

.error-msg { margin-top: 10px; font-size: 12px; color: #e57373; }
.no-task { color: var(--text-muted); font-size: 13px; }

.log-panel { font-family: monospace; font-size: 12px; }
.log-line { padding: 3px 0; border-bottom: 1px solid var(--border); color: var(--text-sub); }
.muted { color: var(--text-muted); }

@media (max-width: 900px) { .backup-layout { grid-template-columns: 1fr; } }
</style>
