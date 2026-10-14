import { relaunch } from '@tauri-apps/plugin-process';
import { check, type DownloadEvent, type Update } from '@tauri-apps/plugin-updater';

export type UpdateStatus =
  | 'idle'
  | 'checking'
  | 'available'
  | 'not-available'
  | 'downloading'
  | 'installed'
  | 'error';

export interface UpdateInfo {
  version: string;
  currentVersion: string;
  date?: string;
  body?: string;
}

export interface UpdateProgress {
  downloadedBytes: number;
  contentLength: number | null;
}

export interface DownloadUpdateResult {
  downloadedBytes: number;
  contentLength: number | null;
}

/** Check configured Tauri update endpoint for available updates. */
export async function checkForAppUpdate(): Promise<Update | null> {
  return check();
}

/** Map Tauri update object to streamlined structure needed by UI. */
export function toUpdateInfo(update: Update): UpdateInfo {
  return {
    version: update.version,
    currentVersion: update.currentVersion,
    date: update.date,
    body: update.body,
  };
}

/** Download and install update while reporting byte progress to UI. */
export async function downloadAndInstallAppUpdate(
  update: Update,
  onProgress: (progress: UpdateProgress) => void,
): Promise<DownloadUpdateResult> {
  let downloadedBytes = 0;
  let contentLength: number | null = null;

  await update.downloadAndInstall((event: DownloadEvent) => {
    if (event.event === 'Started') {
      downloadedBytes = 0;
      contentLength = event.data.contentLength ?? null;
      onProgress({ downloadedBytes, contentLength });
      return;
    }

    if (event.event === 'Progress') {
      downloadedBytes += event.data.chunkLength;
      onProgress({ downloadedBytes, contentLength });
    }
  });

  return { downloadedBytes, contentLength };
}

/** Relaunch desktop application after update installation completes. */
export function relaunchApp(): Promise<void> {
  return relaunch();
}
