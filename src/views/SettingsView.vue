<script setup lang="ts">
import {ref, onMounted} from "vue";
import {
  NCard, NButton, NInput, NInputGroup, NSwitch, NFormItem,
  NModal, NSpin, NCheckbox, NTabs, NTabPane, NSelect, NTag
} from "naive-ui";
import {invoke} from "@tauri-apps/api/core";
import {open} from "@tauri-apps/plugin-dialog";
import type {SaveProfile} from "../types";

const activeTab = ref("general");
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
  saveProfiles: SaveProfile[];
  encryptByDefault: boolean;
  encryptionPassword: string;
  useEncryptPwForRestore: boolean;
  decryptionPassword: string;
  debugMode: boolean;
  closeAction: string;
  compressEnabled: boolean;
  compressLevel: number;
}>();

const emit = defineEmits<{
  "update:baseUrl": [v: string];
  "update:username": [v: string];
  "update:webdavPassword": [v: string];
  "update:remoteRoot": [v: string];
  "update:encryptByDefault": [v: boolean];
  "update:encryptionPassword": [v: string];
  "update:useEncryptPwForRestore": [v: boolean];
  "update:decryptionPassword": [v: string];
  "update:debugMode": [v: boolean];
  "update:closeAction": [v: string];
  "update:compressConfig": [enabled: boolean, level: number];
  saveWebdav: [];
  saveEncryption: [];
  addProfile: [profile: SaveProfile];
  updateProfile: [oldName: string, profile: SaveProfile];
  deleteProfile: [name: string];
}>();

// Profile modal
const showProfileModal = ref(false);
const editingProfileName = ref<string | null>(null); // null = 新增
const profileForm = ref<SaveProfile>({ name: "", saveRoot: "", saveMode: "file", saveExtension: "" });

function openAddProfile() {
  editingProfileName.value = null;
  profileForm.value = { name: "", saveRoot: "", saveMode: "file", saveExtension: "" };
  showProfileModal.value = true;
}
function openEditProfile(p: SaveProfile) {
  editingProfileName.value = p.name;
  profileForm.value = { ...p };
  showProfileModal.value = true;
}
async function pickProfileDir() {
  const dir = await open({ directory: true, multiple: false, title: "选择存档目录" });
  if (dir) profileForm.value.saveRoot = dir as string;
}
function saveProfile() {
  if (!profileForm.value.name.trim() || !profileForm.value.saveRoot.trim()) return;
  if (editingProfileName.value != null) {
    emit("updateProfile", editingProfileName.value, { ...profileForm.value });
  } else {
    emit("addProfile", { ...profileForm.value });
  }
  showProfileModal.value = false;
}

// WebDAV test
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

function onSaveWebdav() { emit('saveWebdav'); }
function openDevtools() { invoke("open_devtools"); }
function openLogDir() { invoke("open_log_dir"); }

async function runTest() {
  showModal.value = true;
  testing.value = true;
  testResult.value = null;
  testError.value = "";
  try { testResult.value = await invoke<TestResult>("test_webdav_connection"); }
  catch (e) { testError.value = String(e); }
  finally { testing.value = false; }
}
</script>

<template>
  <div>
    <n-card>
      <n-tabs v-model:value="activeTab" type="line" animated>
        <!-- 通用设置 Tab -->
        <n-tab-pane name="general" tab="通用设置">
          <div class="tab-content">
            <div class="setting-row">
              <span class="setting-label">关闭窗口时</span>
              <n-select :value="closeAction" style="width:200px"
                        :options="[
                { label: '每次询问', value: 'ask' },
                { label: '最小化到托盘', value: 'minimize' },
                { label: '直接退出', value: 'quit' },
              ]"
                        @update:value="emit('update:closeAction', $event)"/>
            </div>
            <div class="setting-row">
              <span class="setting-label">压缩存档</span>
              <n-switch :value="compressEnabled"
                        @update:value="(v: boolean) => emit('update:compressConfig', v, compressLevel)"/>
              <span class="setting-hint">关闭后备份时不压缩，文件体积更大但速度更快</span>
            </div>
            <div class="setting-row" v-if="compressEnabled">
              <span class="setting-label">压缩等级</span>
              <n-select :value="compressLevel" style="width:200px"
                        :options="[
                { label: '1 - 最快', value: 1 },
                { label: '3 - 快速', value: 3 },
                { label: '6 - 均衡（默认）', value: 6 },
                { label: '12 - 高压缩', value: 12 },
                { label: '19 - 最高压缩', value: 19 },
              ]"
                        @update:value="(v: number) => emit('update:compressConfig', compressEnabled, v)"/>
              <span class="setting-hint">等级越高压缩率越好，但速度越慢</span>
            </div>
          </div>
        </n-tab-pane>

        <!-- 存档设置 Tab -->
        <n-tab-pane name="save" tab="存档配置">
          <div class="tab-content">
            <div class="profile-list">
              <div v-for="p in saveProfiles" :key="p.name" class="profile-card">
                <div class="profile-info">
                  <span class="profile-name">{{ p.name }}</span>
                  <span class="profile-dir">{{ p.saveRoot }}</span>
                  <div class="profile-tags">
                    <n-tag size="tiny" round>{{ p.saveMode === 'folder' ? '文件夹' : '单文件' }}</n-tag>
                    <n-tag v-if="p.saveExtension" size="tiny" round>.{{ p.saveExtension.replace(/^\./, '') }}</n-tag>
                  </div>
                </div>
                <div class="profile-actions">
                  <n-button text size="small" @click="openEditProfile(p)"><i class="fas fa-pen"></i></n-button>
                  <n-button text size="small" type="error" @click="emit('deleteProfile', p.name)"><i class="fas fa-trash"></i></n-button>
                </div>
              </div>
            </div>
            <n-button dashed style="width:100%;margin-top:10px" @click="openAddProfile">
              <i class="fas fa-plus" style="margin-right:6px"></i>添加配置
            </n-button>
          </div>
        </n-tab-pane>

        <!-- WebDAV Tab -->
        <n-tab-pane name="webdav" tab="WebDAV 连接">
          <div class="tab-content">
            <n-form-item label="服务器地址">
              <n-input :value="baseUrl" placeholder="https://dav.example.com"
                       @update:value="emit('update:baseUrl', $event)"/>
            </n-form-item>
            <n-form-item label="用户名">
              <n-input :value="username" placeholder="用户名" @update:value="emit('update:username', $event)"/>
            </n-form-item>
            <n-form-item label="密码">
              <n-input :value="webdavPassword" type="password"
                       :placeholder="webdavPasswordSet && !webdavPassword ? '已保存（输入新密码以修改）' : '密码'"
                       @update:value="emit('update:webdavPassword', $event)"/>
            </n-form-item>
            <n-form-item label="远端目录">
              <n-input :value="remoteRoot" placeholder="/dyson_backup/"
                       @update:value="emit('update:remoteRoot', $event)"/>
            </n-form-item>
            <div class="row-btn">
              <n-button @click="runTest"><i class="fas fa-plug" style="margin-right:4px"></i>测试连接</n-button>
              <n-button type="primary" @click="onSaveWebdav"><i class="fas fa-save" style="margin-right:4px"></i>保存
              </n-button>
            </div>
          </div>
        </n-tab-pane>

        <!-- 加密 Tab -->
        <n-tab-pane name="encrypt" tab="加密设置">
          <div class="tab-content">
            <div class="setting-row">
              <span class="setting-label">默认加密</span>
              <n-switch :value="encryptByDefault" @update:value="emit('update:encryptByDefault', $event)"/>
              <span class="setting-hint">开启后备份时自动加密</span>
            </div>
            <div class="setting-row">
              <span class="setting-label">加密密码</span>
              <n-input :value="encryptionPassword" type="password" placeholder="可选"
                       style="max-width:260px" @update:value="emit('update:encryptionPassword', $event)"/>
            </div>
            <div class="setting-row">
              <span class="setting-label">恢复时使用加密密码</span>
              <n-checkbox :checked="useEncryptPwForRestore"
                          @update:checked="emit('update:useEncryptPwForRestore', $event)"/>
              <span class="setting-hint">勾选后恢复加密备份时自动使用上方密码</span>
            </div>
            <div class="setting-row">
              <span class="setting-label">解密密码</span>
              <n-input :value="decryptionPassword" type="password"
                       :disabled="useEncryptPwForRestore"
                       :placeholder="useEncryptPwForRestore ? '使用加密密码' : '恢复加密备份时使用'"
                       style="max-width:260px" @update:value="emit('update:decryptionPassword', $event)"/>
            </div>
            <div class="row-btn" style="margin-top:12px">
              <n-button type="primary" @click="emit('saveEncryption')"><i class="fas fa-save"
                                                                          style="margin-right:4px"></i>保存
              </n-button>
            </div>
          </div>
        </n-tab-pane>

        <!-- 调试 Tab -->
        <n-tab-pane name="debug" tab="调试">
          <div class="tab-content">
            <div class="setting-row">
              <span class="setting-label">调试模式</span>
              <n-switch :value="debugMode" @update:value="emit('update:debugMode', $event)"/>
              <span class="setting-hint">开启后输出详细日志到配置目录下的 log 文件夹</span>
            </div>
            <div class="row-btn" style="margin-top:12px">
              <n-button @click="openDevtools"><i class="fas fa-code" style="margin-right:4px"></i>开发者工具</n-button>
              <n-button @click="openLogDir"><i class="fas fa-folder-open" style="margin-right:4px"></i>打开日志目录
              </n-button>
              <n-button type="error" @click="showClearConfirm = true"><i class="fas fa-trash"
                                                                         style="margin-right:4px"></i>清空日志（{{
                  formatSize(logSize)
                }}）
              </n-button>
            </div>
          </div>
        </n-tab-pane>
      </n-tabs>
    </n-card>

    <!-- 测试连接弹窗 -->
    <n-modal v-model:show="showModal" preset="card" title="测试 WebDAV 连接" style="width:420px"
             :mask-closable="!testing">
      <div v-if="testing" class="test-loading">
        <n-spin size="medium"/>
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

    <!-- 添加/编辑配置弹窗 -->
    <n-modal v-model:show="showProfileModal" preset="card"
      :title="editingProfileName ? '编辑配置' : '添加配置'" style="width:460px">
      <n-form-item label="配置名称">
        <n-input v-model:value="profileForm.name" placeholder="建议与游戏名一致，如：戴森球计划" />
      </n-form-item>
      <n-form-item label="存档目录">
        <n-input-group>
          <n-input v-model:value="profileForm.saveRoot" placeholder="C:\Users\...\Save" style="margin-right:10px" />
          <n-button @click="pickProfileDir"><i class="fas fa-folder-open"></i></n-button>
        </n-input-group>
      </n-form-item>
      <n-form-item label="存档形式">
        <n-select v-model:value="profileForm.saveMode" style="width:200px"
          :options="[{ label: '单文件', value: 'file' }, { label: '文件夹', value: 'folder' }]" />
      </n-form-item>
      <n-form-item label="存档扩展名">
        <n-input v-model:value="profileForm.saveExtension" placeholder=".dsv" style="width:200px"
          :disabled="profileForm.saveMode === 'folder'" />
      </n-form-item>
      <div class="row-btn" style="justify-content:flex-end">
        <n-button @click="showProfileModal = false">取消</n-button>
        <n-button type="primary" @click="saveProfile"><i class="fas fa-save" style="margin-right:4px"></i>保存</n-button>
      </div>
    </n-modal>
  </div>
</template>

<style scoped>
.tab-content {
  padding: 8px 0;
}

.row-btn {
  display: flex;
  gap: 10px;
}

/* 加密设置水平对齐行 */
.setting-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 0;
  border-bottom: 1px solid var(--border);
}

.setting-label {
  width: 160px;
  flex-shrink: 0;
  font-size: 13px;
  font-weight: 500;
  color: var(--text);
}

.setting-hint {
  font-size: 12px;
  color: var(--text-muted);
  margin-left: auto;
}

/* 测试弹窗 */
.test-loading {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 0;
  color: var(--text-sub);
}

.test-row {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 10px 0;
  border-bottom: 1px solid var(--border);
}

.test-row i {
  font-size: 16px;
  margin-top: 2px;
  flex-shrink: 0;
}

.test-row.ok i {
  color: #18a058;
}

.test-row.fail i {
  color: #d03050;
}

.test-row.warn i {
  color: #f0a020;
}

.test-label {
  font-size: 13px;
  font-weight: 500;
  color: var(--text);
}

.test-msg {
  font-size: 12px;
  color: var(--text-sub);
  margin-top: 2px;
}

.test-overall {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 14px;
  font-weight: 500;
  font-size: 14px;
}

.test-overall.ok {
  color: #18a058;
}

.test-overall.fail {
  color: #d03050;
}

/* Profile 列表 */
.profile-list { display: flex; flex-direction: column; gap: 8px; }
.profile-card {
  display: flex; align-items: center; justify-content: space-between;
  padding: 10px 14px; border-radius: 8px;
  background: var(--bg-item); border: 1px solid var(--border);
}
.profile-info { display: flex; flex-direction: column; gap: 4px; }
.profile-name { font-size: 14px; font-weight: 500; color: var(--text); }
.profile-dir { font-size: 12px; color: var(--text-muted); }
.profile-tags { display: flex; gap: 4px; margin-top: 2px; }
.profile-actions { display: flex; gap: 6px; }
</style>
