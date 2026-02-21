<script setup lang="ts">
import { NCard, NGrid, NGi, NStatistic, NProgress, NTag, NTimeline, NTimelineItem } from "naive-ui";
import type { RemoteBackup } from "../types";
import { formatSize } from "../utils/format";

defineProps<{
  latestBackup: RemoteBackup | null;
  totalLocalSize: number;
  totalCloudSize: number;
  backupCount: number;
  encryptedCount: number;
  chunkedCount: number;
  logs: string[];
}>();
</script>

<template>
  <div class="overview">
    <n-grid :cols="3" :x-gap="16" :y-gap="16" responsive="screen" :collapsed-cols="1" style="align-items:stretch">
      <n-gi style="display:flex">
        <n-card style="flex:1">
          <div class="stat-label"><i class="fas fa-clock"></i> 最近备份状态</div>
          <div class="stat-value">{{ latestBackup ? latestBackup.createdAt.slice(0, 16).replace("T", " ") : "暂无" }}</div>
          <div class="stat-sub">
            <n-tag v-if="latestBackup" type="success" size="small" round>成功</n-tag>
            <n-tag v-if="latestBackup?.profileName" type="info" size="small" round>{{ latestBackup.profileName }}</n-tag>
            <span class="sub-text">{{ latestBackup ? latestBackup.saveName : "尚未备份" }}</span>
          </div>
          <n-progress type="line" :percentage="latestBackup ? 100 : 0" :show-indicator="false" style="margin-top:14px" />
        </n-card>
      </n-gi>
      <n-gi style="display:flex">
        <n-card style="flex:1">
          <div class="stat-label"><i class="fas fa-database"></i> 容量统计</div>
          <div class="dual-stat">
            <n-statistic label="云端使用" :value="formatSize(totalCloudSize)" />
            <n-statistic label="本地存档" :value="formatSize(totalLocalSize)" />
          </div>
          <n-progress type="line" :percentage="totalCloudSize > 0 ? Math.min(100, Math.round(totalCloudSize / (10 * 1073741824) * 100)) : 0" :show-indicator="false" style="margin-top:14px" />
          <div class="stat-sub" style="margin-top:8px"><span class="sub-text">云端共 {{ backupCount }} 条备份</span></div>
        </n-card>
      </n-gi>
      <n-gi style="display:flex">
        <n-card style="flex:1">
          <div class="stat-label"><i class="fas fa-shield-alt"></i> 加密 / 切片</div>
          <div class="dual-stat">
            <n-statistic label="加密备份" :value="encryptedCount" />
            <n-statistic label="分片备份" :value="chunkedCount" />
          </div>
        </n-card>
      </n-gi>
    </n-grid>

    <n-card style="margin-top:16px">
      <div class="stat-label" style="margin-bottom:12px"><i class="fas fa-list-ul"></i> 最近活动</div>
      <n-timeline>
        <n-timeline-item v-for="line in logs.slice(0, 5)" :key="line" :content="line" type="default" />
        <n-timeline-item v-if="logs.length === 0" content="暂无活动记录" type="default" />
      </n-timeline>
    </n-card>
  </div>
</template>

<style scoped>
.overview { display: flex; flex-direction: column; }
.stat-label { color: var(--text-sub); font-size: 13px; margin-bottom: 12px; }
.stat-value { font-size: 22px; font-weight: 500; color: var(--text); margin-bottom: 8px; }
.stat-sub { display: flex; align-items: center; gap: 8px; margin-top: 6px; }
.sub-text { color: var(--text-muted); font-size: 13px; }
.dual-stat { display: flex; gap: 24px; }
</style>
