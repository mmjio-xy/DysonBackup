export type TabKey = "overview" | "backup" | "restore" | "settings";

export type SaveProfile = {
  name: string;
  saveRoot: string;
  saveMode: string;
  saveExtension: string;
};

export type LocalSaveFile = {
  localFilePath: string;
  relativePath: string;
  saveName: string;
  size: number;
  mtimeUnix: number;
  sha256: string;
};

export type RemoteBackup = {
  saveName: string;
  backupId: string;
  createdAt: string;
  originalSize: number;
  compressedSize: number;
  encrypted: boolean;
  chunked: boolean;
  compressed: boolean;
  sourceRelativePath: string;
  profileName: string;
  isTar: boolean;
};

export type TaskProgress = {
  taskId: string;
  phase: string;
  percent: number;
  bytesDone: number;
  bytesTotal: number;
  message: string;
  speedBps: number;
};

export type TaskDone = {
  taskId: string;
  success: boolean;
  error?: string;
};

export type ConflictFound = {
  taskId: string;
  filePath: string;
  isFolder: boolean;
};

export type FileChanged = {
  path: string;
  kind: string;
};

export type LocalSyncEntry = {
  uploadWebdav: boolean;
  localBackupEnabled: boolean;
  localBackupDir: string;
};
