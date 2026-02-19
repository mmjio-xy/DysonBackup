<script setup lang="ts">
import { ref, onMounted } from "vue";
import { NConfigProvider, NButton, NScrollbar } from "naive-ui";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getVersion } from "@tauri-apps/api/app";
import { useTheme } from "./composables/useTheme";
import { useAppState } from "./composables/useAppState";
import OverviewView from "./views/OverviewView.vue";
import BackupView from "./views/BackupView.vue";
import RestoreView from "./views/RestoreView.vue";
import SettingsView from "./views/SettingsView.vue";
import type { TabKey } from "./types";

const activeTab = ref<TabKey>("overview");
const { naiveTheme, isDark, toggle } = useTheme();
const {
  saveRoot, latestBackup, totalLocalSize, totalCloudSize, backups,
  encryptedCount, chunkedCount, logs, files, taskStatus, lastTaskId,
  restoreTargetDir, encryptionPassword, decryptionPassword, useEncryptPwForRestore, conflictState,
  baseUrl, username, webdavPassword, webdavPasswordSet, remoteRoot, encryptByDefault,
  debugMode, setDebugMode, saveEncryptionSettings,
  scanSaves, startBackup, loadBackups, restore, cancelTask, deleteBackup,
  saveWebDavConfig, detectPaths, savePath, selectRestoreDir, resolveConflict,
} = useAppState();

const tabs: { key: TabKey; label: string; icon: string }[] = [
  { key: "overview", label: "概况", icon: "fas fa-chart-pie" },
  { key: "backup", label: "开始备份", icon: "fas fa-cloud-upload-alt" },
  { key: "restore", label: "恢复存档", icon: "fas fa-cloud-download-alt" },
  { key: "settings", label: "设置", icon: "fas fa-cog" },
];

const appWindow = getCurrentWindow();
const appVersion = ref("");
onMounted(async () => { appVersion.value = await getVersion(); });
</script>

<template>
  <n-config-provider :theme="naiveTheme">
    <div class="shell" :class="isDark ? 'dark' : 'light'">
      <!-- 标题栏 -->
      <div class="titlebar" data-tauri-drag-region>
        <div class="titlebar-title" data-tauri-drag-region>
          <i class="fas fa-cloud-sun title-icon"></i>
          <span>戴森球计划 · 存档备份</span>
          <span class="version">v{{ appVersion }}</span>
        </div>
        <div class="titlebar-btns">
          <n-button text size="small" class="theme-btn" @click="toggle">
            <i :class="isDark ? 'fas fa-sun' : 'fas fa-moon'"></i>
          </n-button>
          <button class="tb-btn" @click="appWindow.minimize()">─</button>
          <button class="tb-btn" @click="appWindow.toggleMaximize()">□</button>
          <button class="tb-btn close" @click="appWindow.close()">✕</button>
        </div>
      </div>

      <!-- 主内容 -->
      <div class="app">
        <n-scrollbar style="max-height:100%">
          <div class="app-inner">
            <nav class="nav-row">
          <button
            v-for="t in tabs" :key="t.key"
            class="nav-card" :class="{ active: activeTab === t.key }"
            @click="activeTab = t.key"
          ><i :class="t.icon"></i>{{ t.label }}</button>
        </nav>

        <OverviewView v-if="activeTab === 'overview'"
          :latest-backup="latestBackup" :total-local-size="totalLocalSize"
          :total-cloud-size="totalCloudSize" :backup-count="backups.length"
          :encrypted-count="encryptedCount" :chunked-count="chunkedCount" :logs="logs" />
        <BackupView v-else-if="activeTab === 'backup'"
          :files="files" :logs="logs" :task-status="taskStatus" :last-task-id="lastTaskId"
          @scan="scanSaves" @backup="startBackup" @cancel="cancelTask" />
        <RestoreView v-else-if="activeTab === 'restore'"
          :backups="backups" :restore-target-dir="restoreTargetDir"
          :save-root="saveRoot"
          :encryption-password="encryptionPassword"
          :task-status="taskStatus" :last-task-id="lastTaskId"
          :conflict-state="conflictState"
          @refresh="loadBackups" @restore="restore"
          :on-delete="deleteBackup" @reload="loadBackups"
          :on-select-dir="selectRestoreDir"
          :on-resolve-conflict="resolveConflict"
          @cancel="cancelTask" />
        <SettingsView v-else-if="activeTab === 'settings'"
          :base-url="baseUrl" :username="username" :webdav-password="webdavPassword"
          :webdav-password-set="webdavPasswordSet"
          :remote-root="remoteRoot" :save-root="saveRoot"
          :encrypt-by-default="encryptByDefault" :encryption-password="encryptionPassword"
          :use-encrypt-pw-for-restore="useEncryptPwForRestore" :decryption-password="decryptionPassword"
          @update:base-url="baseUrl = $event" @update:username="username = $event"
          @update:webdav-password="webdavPassword = $event" @update:remote-root="remoteRoot = $event"
          @update:save-root="saveRoot = $event" @update:encrypt-by-default="encryptByDefault = $event"
          @update:encryption-password="encryptionPassword = $event"
          @update:use-encrypt-pw-for-restore="useEncryptPwForRestore = $event"
          @update:decryption-password="decryptionPassword = $event"
          :debug-mode="debugMode"
          @save-webdav="saveWebDavConfig"
          @save-encryption="saveEncryptionSettings"
          @detect-paths="detectPaths" @save-path="savePath"
          @update:debug-mode="setDebugMode" />
          </div>
        </n-scrollbar>
      </div>
    </div>
  </n-config-provider>
</template>

<style>
* { box-sizing: border-box; }
body { margin: 0; background: transparent; font-family: "Inter", "Microsoft YaHei", system-ui, sans-serif; user-select: none; }

@keyframes fa-spin { 0% { transform: rotate(0deg); } 100% { transform: rotate(360deg); } }
.fa-spin { animation: fa-spin 1s infinite linear; }
.fa-pulse { animation: fa-spin 1s infinite steps(8); }
.spinner-icon { animation: fa-spin 1s infinite linear; }

/* ── 暗色 tokens ── */
.dark {
  --bg-base:    #101014;
  --bg-card:    #18181c;
  --bg-item:    #1e1e24;
  --border:     #2d2d35;
  --border-nav: #3a3a45;
  --text:       #ffffffd9;
  --text-sub:   #ffffff73;
  --text-muted: #ffffff40;
  --accent:     #63e2b7;
  --accent-bg:  #18a05820;
  --titlebar:   #0c0c10;
  --nav-active-bg:     #18a05818;
  --nav-active-border: #18a058;
  --nav-active-text:   #63e2b7;
}

/* ── 亮色 tokens ── */
.light {
  --bg-base:    #f5f5f5;
  --bg-card:    #ffffff;
  --bg-item:    #fafafa;
  --border:     #e0e0e6;
  --border-nav: #d0d0d8;
  --text:       #333640;
  --text-sub:   #666872;
  --text-muted: #999aaa;
  --accent:     #18a058;
  --accent-bg:  #18a05812;
  --titlebar:   #ececf0;
  --nav-active-bg:     #18a05812;
  --nav-active-border: #18a058;
  --nav-active-text:   #18a058;
}
</style>

<style scoped>
.shell {
  height: 100vh; display: flex; flex-direction: column;
  background: var(--bg-base); border-radius: 10px;
  overflow: hidden; border: 1px solid var(--border);
  color: var(--text);
}

.titlebar {
  height: 38px; background: var(--titlebar);
  display: flex; align-items: center; justify-content: space-between;
  padding: 0 6px 0 14px; flex-shrink: 0;
  border-bottom: 1px solid var(--border);
}
.titlebar-title {
  display: flex; align-items: center; gap: 8px;
  font-size: 13px; color: var(--text);
}
.title-icon { color: var(--accent); }
.version {
  background: var(--bg-item); border-radius: 4px;
  padding: 1px 7px; font-size: 11px; color: var(--text-sub);
}
.titlebar-btns { display: flex; align-items: center; gap: 2px; }
.theme-btn { font-size: 15px !important; padding: 0 8px !important; cursor: pointer; }
.tb-btn {
  width: 32px; height: 28px; background: transparent; border: none;
  color: var(--text); border-radius: 4px; cursor: pointer; font-size: 12px;
  display: flex; align-items: center; justify-content: center; transition: background .1s;
}
.tb-btn:hover { background: var(--bg-item); color: var(--text); }
.tb-btn.close:hover { background: #c0392b; color: #fff; }

.app { flex: 1; overflow: hidden; background: var(--bg-base); }
.app-inner { padding: 16px 20px 20px; }

.nav-row { display: flex; gap: 8px; flex-wrap: wrap; margin-bottom: 16px; align-items: center; }
.nav-card {
  background: var(--bg-card); border: 1px solid var(--border-nav);
  color: var(--text-sub); border-radius: 8px;
  padding: 7px 16px; cursor: pointer; font-size: 13px; transition: .12s;
  display: flex; align-items: center; gap: 7px;
}
.nav-card:hover { border-color: var(--accent); color: var(--text); }
.nav-card.active {
  background: var(--nav-active-bg); border-color: var(--nav-active-border);
  color: var(--nav-active-text);
}
.path-tip { margin-left: auto; color: var(--text-muted); font-size: 12px; }
</style>
