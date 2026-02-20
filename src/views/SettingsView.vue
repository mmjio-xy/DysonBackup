<script setup lang="ts">
import { ref, onMounted } from "vue";
import { NCard, NButton, NInput, NSwitch, NFormItem, NModal, NSpin, NCheckbox, NTabs, NTabPane, NSelect } from "naive-ui";
import { invoke } from "@tauri-apps/api/core";

const activeTab = ref("webdav");
const logSize = ref(0);
const showClearConfirm = ref(false);

function formatSize(bytes: number): string {
  if (bytes < 1024) return bytes + " B";
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB";
  return (bytes / 1024 / 1024).toFixed(1) + " MB";
}

async function refreshLogSize() {
  try { logSize.value = await invoke<number>("get_log_size"); } catch { logSize.value = 0; }
}

async function confirmClearLogs() {
  try { await invoke("clear_logs"); } catch { /* ignore */ }
  showClearConfirm.value = false;
  await refreshLogSize();
}

onMounted(refreshLogSize);

defineProps<{
  baseUrl: string;
  username: string;
  webdavPassword: string;
  webdavPasswordSet: boolean;
  remoteRoot: string;
  saveRoot: string;
  encryptByDefault: boolean;
  encryptionPassword: string;
  useEncryptPwForRestore: boolean;
  decryptionPassword: string;
  debugMode: boolean;
  closeAction: string;
}>();

const emit = defineEmits<{
  "update:baseUrl": [v: string];
  "update:username": [v: string];
  "update:webdavPassword": [v: string];
  "update:remoteRoot": [v: string];
  "update:saveRoot": [v: string];
  "update:encryptByDefault": [v: boolean];
  "update:encryptionPassword": [v: string];
  "update:useEncryptPwForRestore": [v: boolean];
  "update:decryptionPassword": [v: string];
  "update:debugMode": [v: boolean];
  "update:closeAction": [v: string];
  saveWebdav: [];
  saveEncryption: [];
  detectPaths: [];
  savePath: [];
}>();

type TestResult = {
  serverReachable: boolean; serverMessage: string;
  authOk: boolean; authMessage: string;
  remoteDirExists: boolean; remoteDirMessage: string;
  overallOk: boolean;
};

const showModal = ref(false);
const testing = ref(false);
const testResult = ref<TestResult | null>(null);
const testError = ref("");

function onSaveWebdav() {
  emit('saveWebdav');
  emit('savePath');
}

function openDevtools() {
  invoke("open_devtools");
}

function openLogDir() {
  invoke("open_log_dir");
}

async function runTest() {
  showModal.value = true;
  testing.value = true;
  testResult.value = null;
  testError.value = "";
  try {
    testResult.value = await invoke<TestResult>("test_webdav_connection");
  } catch (e) {
    testError.value = String(e);
  } finally {
    testing.value = false;
  }
}
</script>

<template>
  <div>
  <n-card>
    <n-tabs v-model:value="activeTab" type="line" animated>
      <!-- WebDAV Tab -->
      <n-tab-pane name="webdav" tab="WebDAV 连接">
        <div class="tab-content">
          <n-form-item label="服务器地址">
            <n-input :value="baseUrl" placeholder="https://dav.example.com" @update:value="emit('update:baseUrl', $event)" />
          </n-form-item>
          <n-form-item label="用户名">
            <n-input :value="username" placeholder="用户名" @update:value="emit('update:username', $event)" />
          </n-form-item>
          <n-form-item label="密码">
            <n-input :value="webdavPassword" type="password"
              :placeholder="webdavPasswordSet && !webdavPassword ? '已保存（输入新密码以修改）' : '密码'"
              @update:value="emit('update:webdavPassword', $event)" />
          </n-form-item>
          <n-form-item label="远端目录">
            <n-input :value="remoteRoot" placeholder="/dyson_backup/" @update:value="emit('update:remoteRoot', $event)" />
          </n-form-item>
          <n-form-item label="存档目录">
            <n-input :value="saveRoot" placeholder="C:\Users\...\Save" @update:value="emit('update:saveRoot', $event)" />
          </n-form-item>
          <div class="row-btn">
            <n-button @click="emit('detectPaths')"><i class="fas fa-search" style="margin-right:4px"></i>探测存档</n-button>
            <n-button @click="runTest"><i class="fas fa-plug" style="margin-right:4px"></i>测试连接</n-button>
            <n-button type="primary" @click="onSaveWebdav"><i class="fas fa-save" style="margin-right:4px"></i>保存</n-button>
          </div>
        </div>
      </n-tab-pane>

      <!-- 加密 Tab -->
      <n-tab-pane name="encrypt" tab="加密设置">
        <div class="tab-content">
          <div class="setting-row">
            <span class="setting-label">默认加密</span>
            <n-switch :value="encryptByDefault" @update:value="emit('update:encryptByDefault', $event)" />
            <span class="setting-hint">开启后备份时自动加密</span>
          </div>
          <div class="setting-row">
            <span class="setting-label">加密密码</span>
            <n-input :value="encryptionPassword" type="password" placeholder="可选"
              style="max-width:260px" @update:value="emit('update:encryptionPassword', $event)" />
          </div>
          <div class="setting-row">
            <span class="setting-label">恢复时使用加密密码</span>
            <n-checkbox :checked="useEncryptPwForRestore" @update:checked="emit('update:useEncryptPwForRestore', $event)" />
            <span class="setting-hint">勾选后恢复加密备份时自动使用上方密码</span>
          </div>
          <div class="setting-row">
            <span class="setting-label">解密密码</span>
            <n-input :value="decryptionPassword" type="password"
              :disabled="useEncryptPwForRestore"
              :placeholder="useEncryptPwForRestore ? '使用加密密码' : '恢复加密备份时使用'"
              style="max-width:260px" @update:value="emit('update:decryptionPassword', $event)" />
          </div>
          <div class="row-btn" style="margin-top:12px">
            <n-button type="primary" @click="emit('saveEncryption')"><i class="fas fa-save" style="margin-right:4px"></i>保存</n-button>
          </div>
        </div>
      </n-tab-pane>

      <!-- 通用 Tab -->
      <n-tab-pane name="general" tab="通用">
        <div class="tab-content">
          <div class="setting-row">
            <span class="setting-label">关闭窗口时</span>
            <n-select :value="closeAction" style="width:200px"
              :options="[
                { label: '每次询问', value: 'ask' },
                { label: '最小化到托盘', value: 'minimize' },
                { label: '直接退出', value: 'quit' },
              ]"
              @update:value="emit('update:closeAction', $event)" />
          </div>
        </div>
      </n-tab-pane>

      <!-- 调试 Tab -->
      <n-tab-pane name="debug" tab="调试">
        <div class="tab-content">
          <div class="setting-row">
            <span class="setting-label">调试模式</span>
            <n-switch :value="debugMode" @update:value="emit('update:debugMode', $event)" />
            <span class="setting-hint">开启后输出详细日志到配置目录下的 log 文件夹</span>
          </div>
          <div class="row-btn" style="margin-top:12px">
            <n-button @click="openDevtools"><i class="fas fa-code" style="margin-right:4px"></i>开发者工具</n-button>
            <n-button @click="openLogDir"><i class="fas fa-folder-open" style="margin-right:4px"></i>打开日志目录</n-button>
            <n-button type="error" @click="showClearConfirm = true"><i class="fas fa-trash" style="margin-right:4px"></i>清空日志（{{ formatSize(logSize) }}）</n-button>
          </div>
        </div>
      </n-tab-pane>
    </n-tabs>
  </n-card>

  <!-- 测试连接弹窗 -->
  <n-modal v-model:show="showModal" preset="card" title="测试 WebDAV 连接" style="width:420px" :mask-closable="!testing">
    <div v-if="testing" class="test-loading">
      <n-spin size="medium" />
      <span>正在检测，请稍候...</span>
    </div>
    <div v-else-if="testError" class="test-row fail">
      <i class="fas fa-times-circle"></i>
      <span>{{ testError }}</span>
    </div>
    <template v-else-if="testResult">
      <div class="test-row" :class="testResult.serverReachable ? 'ok' : 'fail'">
        <i :class="testResult.serverReachable ? 'fas fa-check-circle' : 'fas fa-times-circle'"></i>
        <div>
          <div class="test-label">服务器连通性</div>
          <div class="test-msg">{{ testResult.serverMessage }}</div>
        </div>
      </div>
      <div class="test-row" :class="testResult.authOk ? 'ok' : 'fail'">
        <i :class="testResult.authOk ? 'fas fa-check-circle' : 'fas fa-times-circle'"></i>
        <div>
          <div class="test-label">用户名 / 密码</div>
          <div class="test-msg">{{ testResult.authMessage }}</div>
        </div>
      </div>
      <div class="test-row" :class="testResult.remoteDirExists ? 'ok' : 'warn'">
        <i :class="testResult.remoteDirExists ? 'fas fa-check-circle' : 'fas fa-exclamation-triangle'"></i>
        <div>
          <div class="test-label">远端目录</div>
          <div class="test-msg">{{ testResult.remoteDirMessage }}</div>
        </div>
      </div>
      <div class="test-overall" :class="testResult.overallOk ? 'ok' : 'fail'">
        <i :class="testResult.overallOk ? 'fas fa-circle-check' : 'fas fa-circle-xmark'"></i>
        {{ testResult.overallOk ? '连接成功' : '连接失败' }}
      </div>
    </template>
  </n-modal>

  <!-- 清空日志确认弹窗 -->
  <n-modal v-model:show="showClearConfirm" preset="card" title="确认清空日志" style="width:360px">
    <p style="margin:0 0 16px">确定要删除所有日志文件吗？（共 {{ formatSize(logSize) }}）</p>
    <div class="row-btn" style="justify-content:flex-end">
      <n-button @click="showClearConfirm = false">取消</n-button>
      <n-button type="error" @click="confirmClearLogs"><i class="fas fa-trash" style="margin-right:4px"></i>确认删除</n-button>
    </div>
  </n-modal>
  </div>
</template>

<style scoped>
.tab-content { padding: 8px 0; }
.row-btn { display: flex; gap: 10px; }

/* 加密设置水平对齐行 */
.setting-row {
  display: flex; align-items: center; gap: 12px;
  padding: 10px 0; border-bottom: 1px solid var(--border);
}
.setting-label {
  width: 160px; flex-shrink: 0;
  font-size: 13px; font-weight: 500; color: var(--text);
}
.setting-hint { font-size: 12px; color: var(--text-muted); margin-left: auto; }

/* 测试弹窗 */
.test-loading { display: flex; align-items: center; gap: 12px; padding: 8px 0; color: var(--text-sub); }
.test-row {
  display: flex; align-items: flex-start; gap: 12px;
  padding: 10px 0; border-bottom: 1px solid var(--border);
}
.test-row i { font-size: 16px; margin-top: 2px; flex-shrink: 0; }
.test-row.ok i { color: #18a058; }
.test-row.fail i { color: #d03050; }
.test-row.warn i { color: #f0a020; }
.test-label { font-size: 13px; font-weight: 500; color: var(--text); }
.test-msg { font-size: 12px; color: var(--text-sub); margin-top: 2px; }
.test-overall {
  display: flex; align-items: center; gap: 8px;
  margin-top: 14px; font-weight: 500; font-size: 14px;
}
.test-overall.ok { color: #18a058; }
.test-overall.fail { color: #d03050; }
</style>
