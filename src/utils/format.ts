export function formatSize(size: number): string {
  if (size >= 1073741824) return `${(size / 1073741824).toFixed(2)} GB`;
  if (size >= 1048576) return `${(size / 1048576).toFixed(1)} MB`;
  if (size >= 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${size} B`;
}

export function formatSpeed(bps: number): string {
  if (bps <= 0) return "";
  return `${formatSize(bps)}/s`;
}

export function formatTime(unix: number): string {
  return new Date(unix * 1000).toLocaleString("zh-CN");
}
