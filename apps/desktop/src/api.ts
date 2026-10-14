import { invoke } from '@tauri-apps/api/core';
import type {
  AgentConfig,
  CliIntegrationStatus,
  DoctorReport,
  ExportConfigOptions,
  ImportPreview,
  PaperInfo,
  PrinterInfo,
  TaskHistoryEvent,
  TaskHistoryJob,
  TaskLogEntry,
} from '@/types';

/** Read persisted Agent configuration via Tauri. */
export function getConfig(): Promise<AgentConfig> {
  return invoke<AgentConfig>('get_config');
}

/** Run read-only desktop environment check shared with CLI. */
export function runDoctor(): Promise<DoctorReport> {
  return invoke<DoctorReport>('run_doctor');
}

/** Save Agent configuration via Tauri. */
export function saveConfig(config: AgentConfig): Promise<AgentConfig> {
  return invoke<AgentConfig>('save_config', { config });
}

/** Export encrypted configuration file. */
export function exportConfigFile(
  path: string,
  password: string,
  options: ExportConfigOptions,
): Promise<void> {
  return invoke<void>('export_config_file', { path, password, options });
}

/** Preview changes before importing encrypted configuration file. */
export function previewConfigImport(path: string, password: string): Promise<ImportPreview> {
  return invoke<ImportPreview>('preview_config_import', { path, password });
}

/** Import encrypted configuration file and return saved Agent configuration. */
export function importConfigFile(
  path: string,
  password: string,
  expectedFileHash: string,
): Promise<AgentConfig> {
  return invoke<AgentConfig>('import_config_file', { path, password, expectedFileHash });
}

/** Run connection test using current remote task configuration. */
export function testRemoteConnection(config: AgentConfig): Promise<void> {
  return invoke<void>('test_remote_connection', { config });
}

/** Check whether current desktop app is a debug build. */
export function isDebugBuild(): Promise<boolean> {
  return invoke<boolean>('is_debug_build');
}

export function getCliIntegrationStatus(): Promise<CliIntegrationStatus> {
  return invoke<CliIntegrationStatus>('get_cli_integration_status');
}

export function installCliIntegration(): Promise<CliIntegrationStatus> {
  return invoke<CliIntegrationStatus>('install_cli_integration');
}

export function uninstallCliIntegration(): Promise<CliIntegrationStatus> {
  return invoke<CliIntegrationStatus>('uninstall_cli_integration');
}

/** Read recent task logs retained by local Agent. */
export function getLogs(): Promise<TaskLogEntry[]> {
  return invoke<TaskLogEntry[]>('get_logs');
}

/** Read recent task history. */
export function getTaskHistory(): Promise<TaskHistoryJob[]> {
  return invoke<TaskHistoryJob[]>('get_task_history');
}

/** Read historical events for a specific task. */
export function getTaskHistoryEvents(jobId: string): Promise<TaskHistoryEvent[]> {
  return invoke<TaskHistoryEvent[]>('get_task_history_events', { jobId });
}

/** Clear local task history. */
export function clearTaskHistory(): Promise<void> {
  return invoke<void>('clear_task_history');
}

/** Fetch list of printers via shared command service. */
export function fetchPrinters(): Promise<PrinterInfo[]> {
  return invoke<PrinterInfo[]>('list_printers');
}

/** Fetch paper sizes for a specific printer via shared command service. */
export function fetchPapers(printerName: string): Promise<PaperInfo[]> {
  return invoke<PaperInfo[]>('list_papers', { printerName });
}

/** Submit a test calibration page using current default print settings. */
export function printTestPage(config: AgentConfig): Promise<void> {
  return invoke<void>('print_test', { config });
}
