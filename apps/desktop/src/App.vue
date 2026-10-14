<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  Download,
  CircleHelp,
  ExternalLink,
  FileDown,
  FileUp,
  Monitor,
  Moon,
  Printer,
  RefreshCw,
  RotateCw,
  Save,
  Shuffle,
  Sun,
  Trash2,
  X,
} from '@lucide/vue';
import { getVersion } from '@tauri-apps/api/app';
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog';
import { openUrl } from '@tauri-apps/plugin-opener';
import appIcon from '@/assets/app-icon.png';
import {
  exportConfigFile,
  fetchPapers,
  fetchPrinters,
  clearTaskHistory,
  getConfig,
  getCliIntegrationStatus,
  getTaskHistory,
  getTaskHistoryEvents,
  importConfigFile,
  isDebugBuild,
  installCliIntegration,
  printTestPage,
  previewConfigImport,
  saveConfig,
  testRemoteConnection,
  uninstallCliIntegration,
} from '@/api';
import type {
  AgentConfig,
  CliIntegrationStatus,
  EffectivePaper,
  ExportConfigOptions,
  ImportPreview,
  PaperInfo,
  PrinterInfo,
  TaskHistoryEvent,
  TaskHistoryJob,
  TaskHistorySource,
  TaskHistoryStatus,
} from '@/types';
import {
  checkForAppUpdate,
  downloadAndInstallAppUpdate,
  relaunchApp,
  toUpdateInfo,
  type UpdateInfo,
  type UpdateProgress,
  type UpdateStatus,
} from '@/updater';
import type { Update } from '@tauri-apps/plugin-updater';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { ScrollArea } from '@/components/ui/scroll-area';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Separator } from '@/components/ui/separator';
import { Switch } from '@/components/ui/switch';
import {
  Table,
  TableBody,
  TableCell,
  TableEmpty,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import CliIntegrationRow from '@/components/CliIntegrationRow.vue';
import StatusDoctorSheet from '@/components/StatusDoctorSheet.vue';
import { DEFAULT_UI_LANGUAGE, isUiLanguage, setI18nLocale, type UiLanguage } from '@/i18n';
import { useOnboarding } from '@/onboarding';

const DEFAULT_PAPER: EffectivePaper = {
  width_mm: 60,
  height_mm: 40,
};
const DEFAULT_REMOTE_CONFIG: AgentConfig['remote'] = {
  enabled: false,
  endpoint_url: null,
  bearer_token: null,
  device_id: null,
  device_name: null,
  poll_interval_seconds: 10,
  max_report_retries: 10,
  history_retention_days: 3,
};

const GITHUB_REPOSITORY_URL = 'https://github.com/vergil-lai/print-bridge';
const GITHUB_RELEASES_URL = 'https://github.com/vergil-lai/print-bridge/releases';
const MIN_SERVICE_PORT = 10000;
const MAX_SERVICE_PORT = 65535;
const MIN_REMOTE_POLL_INTERVAL_SECONDS = 3;
const MIN_REMOTE_MAX_REPORT_RETRIES = 1;
const REQUIRED_LOOPBACK_IP = '127.0.0.1';
const THEME_STORAGE_KEY = 'printbridge.theme';
type ThemeMode = 'system' | 'light' | 'dark';

const DEFAULT_APP_CONFIG: AgentConfig['app'] = {
  autostart: false,
  language: DEFAULT_UI_LANGUAGE,
};

const config = ref<AgentConfig | null>(null);
const activeTab = ref('settings');
const doctorOpen = ref(false);
const doctorNeedsAttention = ref(false);
const printers = ref<PrinterInfo[]>([]);
const papers = ref<PaperInfo[]>([]);
const taskHistory = ref<TaskHistoryJob[]>([]);
const selectedTaskJobId = ref<string | null>(null);
const selectedTaskEvents = ref<TaskHistoryEvent[]>([]);
const themeMode = ref<ThemeMode>(readThemeMode());
const originDraft = ref('');
const originErrorMessage = ref('');
const ipDraft = ref('');
const ipErrorMessage = ref('');
const errorMessage = ref('');
const successMessage = ref('');
const loadingConfig = ref(true);
const loadingPrinters = ref(false);
const loadingPapers = ref(false);
const loadingTaskHistory = ref(false);
const loadingTaskEvents = ref(false);
const clearingTaskHistory = ref(false);
const confirmingClearTaskHistory = ref(false);
const saving = ref(false);
const savingWhitelist = ref(false);
const exportingConfig = ref(false);
const importingConfig = ref(false);
const previewingConfigImport = ref(false);
const showExportDialog = ref(false);
const showImportDialog = ref(false);
const showEmptyPasswordTokenConfirm = ref(false);
const exportPassword = ref('');
const importPassword = ref('');
const importPath = ref('');
const importPreview = ref<ImportPreview | null>(null);
const importErrorMessage = ref('');
const exportOptions = ref<ExportConfigOptions>(defaultExportOptions());
const testingPrint = ref(false);
const testingRemote = ref(false);
const cliIntegrationStatus = ref<CliIntegrationStatus | null>(null);
const changingCliIntegration = ref(false);
const activePort = ref<number | null>(null);
const appVersion = ref('-');
const updateStatus = ref<UpdateStatus>('idle');
const updateMessage = ref('');
const availableUpdate = shallowRef<Update | null>(null);
const updateInfo = ref<UpdateInfo | null>(null);
const updateProgress = ref<UpdateProgress>({
  downloadedBytes: 0,
  contentLength: null,
});
let colorSchemeQuery: MediaQueryList | null = null;
let taskHistoryRequestId = 0;
let taskEventsRequestId = 0;

const { t } = useI18n();
const onboardingReady = computed(() => config.value !== null && !loadingConfig.value);
const { replayOnboarding } = useOnboarding({
  activeTab,
  ready: onboardingReady,
  t: (key) => t(key),
});

/** Local service port currently requested by the UI. */
const servicePort = computed(() => activePort.value ?? config.value?.service.port ?? 0);
/** Whether saved configured port has not yet taken effect in current running service. */
const hasPendingPortChange = computed(
  () =>
    activePort.value !== null &&
    config.value !== null &&
    config.value.service.port !== activePort.value,
);
/** Human-readable status displayed in top status bar. */
const statusLabel = computed(() => {
  if (loadingConfig.value) return t('loading');
  if (errorMessage.value || doctorNeedsAttention.value) return t('needsAttention');
  return t('ready');
});
/** Compute status badge style based on current error condition. */
const statusVariant = computed(() => {
  if (loadingConfig.value) return 'secondary';
  if (errorMessage.value || doctorNeedsAttention.value) return 'destructive';
  return 'success';
});
/** Only display Save button on settings pages that require explicit submission. */
const showsSaveButton = computed(
  () => activeTab.value === 'settings' || activeTab.value === 'remote',
);
/** Whether the updater is currently checking for a new version. */
const checkingUpdate = computed(() => updateStatus.value === 'checking');
/** Whether an update is currently being downloaded or installed. */
const installingUpdate = computed(() => updateStatus.value === 'downloading');
/** Update download and installation progress percentage. */
const updateProgressPercent = computed(() => {
  const contentLength = updateProgress.value.contentLength;
  if (!contentLength) return 0;

  return Math.min(100, Math.round((updateProgress.value.downloadedBytes / contentLength) * 100));
});
/** Download progress text displayed during update installation. */
const updateProgressText = computed(() => {
  const { downloadedBytes, contentLength } = updateProgress.value;
  if (!contentLength) return formatBytes(downloadedBytes);

  return `${formatBytes(downloadedBytes)} / ${formatBytes(contentLength)}`;
});
/** Current version displayed in the update panel. */
const currentAppVersion = computed(() => updateInfo.value?.currentVersion ?? appVersion.value);
/** Available version displayed in the update panel. */
const availableUpdateVersion = computed(() => updateInfo.value?.version ?? '-');
/** Generate primary button label based on current update status. */
const updateButtonLabel = computed(() => {
  if (installingUpdate.value) return t('updating');
  if (updateInfo.value?.version) return `${t('updateToVersion')} v${updateInfo.value.version}`;

  return t('downloadAndInstall');
});
/** Whether current configuration is sufficient to submit a test print. */
const canTestPrint = computed(
  () =>
    Boolean(config.value?.printing.default_printer) &&
    Boolean(config.value?.printing.default_paper),
);
/** Two-way computed value for default printer selection. */
const selectedPrinter = computed({
  get: () => config.value?.printing.default_printer ?? '',
  set: (value: string) => {
    if (!config.value) return;
    config.value.printing.default_printer = value || null;
  },
});

/** Two-way computed value for paper preset or custom paper selection. */
const selectedPaper = computed({
  get: () => {
    const currentPaper = config.value?.printing.default_paper;
    if (!currentPaper) return 'custom';

    return matchingPaper(currentPaper)?.id ?? 'custom';
  },
  set: (paperId: string) => {
    const paper = papers.value.find((item) => item.id === paperId);
    if (!paper || !config.value) return;

    config.value.printing.default_paper = {
      width_mm: paper.width_mm,
      height_mm: paper.height_mm,
    };
  },
});
/** Summary of currently selected task. */
const selectedTask = computed(
  () => taskHistory.value.find((item) => item.job_id === selectedTaskJobId.value) ?? null,
);

/** Currently configured UI language. */
const currentLanguage = computed(() => config.value?.app.language ?? DEFAULT_APP_CONFIG.language);

watch(currentLanguage, (language) => setI18nLocale(language), { immediate: true });

/** Ensure loaded configuration always has a valid default paper object. */
function normalizeConfig(value: AgentConfig): AgentConfig {
  return {
    ...value,
    app: {
      ...DEFAULT_APP_CONFIG,
      ...value.app,
      language: isUiLanguage(value.app?.language)
        ? value.app.language
        : DEFAULT_APP_CONFIG.language,
    },
    security: {
      ...value.security,
      allowed_ips: normalizeAllowedIps(value.security.allowed_ips ?? []),
    },
    printing: {
      ...value.printing,
      default_paper: value.printing.default_paper ?? { ...DEFAULT_PAPER },
    },
    remote: {
      ...DEFAULT_REMOTE_CONFIG,
      ...value.remote,
    },
  };
}

function defaultExportOptions(): ExportConfigOptions {
  return {
    service_port: true,
    allowed_origins: true,
    allowed_ips: true,
    remote_enabled: true,
    remote_endpoint_url: true,
    remote_bearer_token: true,
    remote_poll_interval_seconds: true,
    remote_max_report_retries: true,
  };
}

/** Determine whether stored string is a supported theme mode. */
function isThemeMode(value: string | null): value is ThemeMode {
  return value === 'system' || value === 'light' || value === 'dark';
}

/** Read saved theme mode from local storage. */
function readThemeMode(): ThemeMode {
  if (typeof window === 'undefined') return 'system';

  const storedMode = window.localStorage.getItem(THEME_STORAGE_KEY);
  return isThemeMode(storedMode) ? storedMode : 'system';
}

/** Apply selected or system-derived theme mode to page document. */
function applyTheme(mode: ThemeMode): void {
  if (typeof window === 'undefined') return;

  const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
  const shouldUseDark = mode === 'dark' || (mode === 'system' && prefersDark);

  document.documentElement.classList.toggle('dark', shouldUseDark);
  document.documentElement.style.colorScheme = shouldUseDark ? 'dark' : 'light';
}

/** Save selected theme mode and apply immediately. */
function setThemeMode(value: string): void {
  const nextMode = isThemeMode(value) ? value : 'system';

  themeMode.value = nextMode;
  window.localStorage.setItem(THEME_STORAGE_KEY, nextMode);
  applyTheme(nextMode);
}

/** Update UI language and persist with saved configuration. */
function setLanguage(value: string): void {
  if (!config.value) return;
  config.value.app.language = isUiLanguage(value) ? value : DEFAULT_APP_CONFIG.language;
}

/** Generate CSS class for individual theme option button. */
function segmentedOptionClass(isActive: boolean): string {
  return [
    'inline-flex h-8 items-center justify-center gap-2 rounded-md px-3 text-sm font-medium transition-colors',
    'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background',
    isActive
      ? 'bg-primary text-primary-foreground shadow-sm'
      : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
  ].join(' ');
}

function themeOptionClass(mode: ThemeMode): string {
  return segmentedOptionClass(themeMode.value === mode);
}

function languageOptionClass(language: UiLanguage): string {
  return segmentedOptionClass(currentLanguage.value === language);
}

/** Re-apply system theme when system color scheme changes. */
function handleSystemThemeChange(): void {
  if (themeMode.value === 'system') {
    applyTheme('system');
  }
}

/** Register system color scheme listener for system theme mode. */
function setupThemeSync(): void {
  if (typeof window === 'undefined') return;

  colorSchemeQuery = window.matchMedia('(prefers-color-scheme: dark)');
  colorSchemeQuery.addEventListener('change', handleSystemThemeChange);
}

/** Return editable paper config, creating default paper when necessary. */
function currentPaper(): EffectivePaper {
  if (!config.value) return DEFAULT_PAPER;
  if (!config.value.printing.default_paper) {
    config.value.printing.default_paper = { ...DEFAULT_PAPER };
  }

  return config.value.printing.default_paper;
}

/** Find printer paper preset matching specified paper dimensions. */
function matchingPaper(paper: EffectivePaper): PaperInfo | undefined {
  return papers.value.find(
    (item) =>
      Math.abs(item.width_mm - paper.width_mm) < 0.01 &&
      Math.abs(item.height_mm - paper.height_mm) < 0.01,
  );
}

/** Update service port in configuration when input value is valid. */
function setPort(value: string | number): void {
  if (!config.value) return;
  const port = Number(value);
  if (Number.isInteger(port)) {
    config.value.service.port = Math.min(MAX_SERVICE_PORT, Math.max(MIN_SERVICE_PORT, port));
  }
}

/** Update custom paper dimension when input value is valid. */
function setPaperDimension(key: keyof EffectivePaper, value: string | number): void {
  const dimension = Number(value);
  if (Number.isFinite(dimension) && dimension > 0) {
    currentPaper()[key] = dimension;
  }
}

/** Update nullable string field in remote task configuration. */
function setRemoteString(
  key: 'endpoint_url' | 'bearer_token' | 'device_id' | 'device_name',
  value: string | number,
): void {
  if (!config.value) return;
  const nextValue = String(value).trim();
  config.value.remote[key] = nextValue || null;
}

/** Update positive integer field in remote task configuration. */
function setRemoteNumber(
  key: 'poll_interval_seconds' | 'max_report_retries',
  value: string | number,
): void {
  if (!config.value) return;
  const nextValue = Number(value);
  if (Number.isInteger(nextValue)) {
    const minValue =
      key === 'poll_interval_seconds'
        ? MIN_REMOTE_POLL_INTERVAL_SECONDS
        : MIN_REMOTE_MAX_REPORT_RETRIES;
    config.value.remote[key] = Math.max(minValue, nextValue);
  }
}

/** Dismiss top alert banner message. */
function dismissMessage(kind: 'error' | 'success'): void {
  if (kind === 'error') {
    errorMessage.value = '';
  } else {
    successMessage.value = '';
  }
}

function openExportDialog(): void {
  exportOptions.value = defaultExportOptions();
  exportPassword.value = '';
  showEmptyPasswordTokenConfirm.value = false;
  showExportDialog.value = true;
}

async function handleExportConfig(): Promise<void> {
  if (exportOptions.value.remote_bearer_token && exportPassword.value.length === 0) {
    showEmptyPasswordTokenConfirm.value = true;
    return;
  }

  await runExportConfig();
}

async function confirmEmptyPasswordExport(): Promise<void> {
  showEmptyPasswordTokenConfirm.value = false;
  await runExportConfig();
}

async function runExportConfig(): Promise<void> {
  exportingConfig.value = true;
  errorMessage.value = '';
  successMessage.value = '';

  try {
    const path = await saveDialog({
      defaultPath: 'printbridge-config.json',
      filters: [{ name: 'PrintBridge Config', extensions: ['json'] }],
    });
    if (!path) return;

    await exportConfigFile(path, exportPassword.value, exportOptions.value);
    showExportDialog.value = false;
    successMessage.value = t('configExported');
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : t('exportConfigFailed');
  } finally {
    exportingConfig.value = false;
  }
}

function openImportDialog(): void {
  importPassword.value = '';
  importPath.value = '';
  importPreview.value = null;
  importErrorMessage.value = '';
  showImportDialog.value = true;
}

function resetImportPreview(): void {
  importPreview.value = null;
  importErrorMessage.value = '';
}

async function chooseImportFile(): Promise<void> {
  const path = await openDialog({
    multiple: false,
    filters: [{ name: 'PrintBridge Config', extensions: ['json'] }],
  });
  if (typeof path === 'string') {
    importPath.value = path;
    resetImportPreview();
  }
}

async function handlePreviewConfigImport(): Promise<void> {
  if (!importPath.value) return;
  previewingConfigImport.value = true;
  importErrorMessage.value = '';
  errorMessage.value = '';
  successMessage.value = '';

  try {
    importPreview.value = await previewConfigImport(importPath.value, importPassword.value);
  } catch (error) {
    importPreview.value = null;
    importErrorMessage.value = error instanceof Error ? error.message : t('importPreviewFailed');
  } finally {
    previewingConfigImport.value = false;
  }
}

async function handleImportConfig(): Promise<void> {
  if (!importPath.value || !importPreview.value) return;
  importingConfig.value = true;
  importErrorMessage.value = '';
  errorMessage.value = '';
  successMessage.value = '';

  try {
    config.value = normalizeConfig(
      await importConfigFile(importPath.value, importPassword.value, importPreview.value.file_hash),
    );
    showImportDialog.value = false;
    successMessage.value = t('configImported');
  } catch (error) {
    importErrorMessage.value = error instanceof Error ? error.message : t('importConfigFailed');
  } finally {
    importingConfig.value = false;
  }
}

/** Generate random UUID v4 for current device. */
function generateRemoteDeviceId(): void {
  if (!config.value) return;
  config.value.remote.device_id = crypto.randomUUID();
}

/** Load configuration, printers, papers, and task history to initialize page state. */
async function loadConfig(): Promise<void> {
  loadingConfig.value = true;
  errorMessage.value = '';

  try {
    config.value = normalizeConfig(await getConfig());
    activePort.value = config.value.service.port;
    await Promise.all([refreshPrinters(), refreshTaskHistory()]);
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : t('loadConfigFailed');
  } finally {
    loadingConfig.value = false;
  }
}

/** Read current desktop application version from Tauri. */
async function loadAppVersion(): Promise<void> {
  try {
    appVersion.value = await getVersion();
  } catch {
    appVersion.value = '-';
  }
}

/** Refresh printer list and select appropriate default printer if not configured. */
async function refreshPrinters(): Promise<void> {
  if (!config.value) return;
  loadingPrinters.value = true;
  errorMessage.value = '';

  try {
    printers.value = await fetchPrinters();
    if (!config.value.printing.default_printer) {
      config.value.printing.default_printer =
        printers.value.find((printer) => printer.is_default)?.name ??
        printers.value[0]?.name ??
        null;
    }
    await refreshPapers();
  } catch (error) {
    printers.value = [];
    papers.value = [];
    errorMessage.value = error instanceof Error ? error.message : t('refreshPrintersFailed');
  } finally {
    loadingPrinters.value = false;
  }
}

/** Refresh paper options for currently selected printer. */
async function refreshPapers(): Promise<void> {
  if (!config.value?.printing.default_printer) {
    papers.value = [];
    return;
  }

  loadingPapers.value = true;
  errorMessage.value = '';

  try {
    papers.value = await fetchPapers(config.value.printing.default_printer);
  } catch (error) {
    papers.value = [];
    errorMessage.value = error instanceof Error ? error.message : t('refreshPapersFailed');
  } finally {
    loadingPapers.value = false;
  }
}

/** Apply printer selection and reload corresponding paper list. */
async function handlePrinterChange(value: string): Promise<void> {
  selectedPrinter.value = value;
  await refreshPapers();
}

/** Save current settings via Tauri; restart application if port changed to apply new listen port. */
async function persistConfig(): Promise<void> {
  if (!config.value) return;
  saving.value = true;
  errorMessage.value = '';
  successMessage.value = '';

  try {
    const savedPort = config.value.service.port;
    const portChanged = activePort.value !== null && savedPort !== activePort.value;
    if (config.value.remote.enabled) {
      await testRemoteConnection(config.value);
    }
    config.value = normalizeConfig(await saveConfig(config.value));
    if (portChanged) {
      if (await isDebugBuild()) {
        successMessage.value = t('settingsSavedDev');
        return;
      }
      successMessage.value = t('settingsSavedRestarting');
      await relaunchApp();
      return;
    }
    successMessage.value = t('settingsSaved');
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : t('saveOrRestartFailed');
  } finally {
    saving.value = false;
  }
}

/** Manually test GET/POST connectivity for remote task URL. */
async function handleTestRemoteConnection(): Promise<void> {
  if (!config.value || !config.value.remote.enabled) return;
  testingRemote.value = true;
  errorMessage.value = '';
  successMessage.value = '';

  try {
    await testRemoteConnection(config.value);
    successMessage.value = t('remoteConnectionPassed');
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : t('remoteConnectionFailed');
  } finally {
    testingRemote.value = false;
  }
}

/** Submit calibration test page using current Agent default print settings. */
async function handleTestPrint(): Promise<void> {
  if (!config.value || !canTestPrint.value) return;
  testingPrint.value = true;
  errorMessage.value = '';
  successMessage.value = '';

  try {
    await printTestPage(config.value);
    successMessage.value = t('testPrintSubmitted');
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : t('testPrintFailed');
  } finally {
    testingPrint.value = false;
  }
}

/** Refresh task history from local Agent. */
async function refreshTaskHistory(): Promise<void> {
  confirmingClearTaskHistory.value = false;
  const requestId = ++taskHistoryRequestId;
  loadingTaskHistory.value = true;

  try {
    const nextTaskHistory = await getTaskHistory();
    if (requestId !== taskHistoryRequestId) return;

    taskHistory.value = nextTaskHistory;
    errorMessage.value = '';
    if (selectedTaskJobId.value) {
      const stillExists = taskHistory.value.some((item) => item.job_id === selectedTaskJobId.value);
      if (stillExists) {
        await selectTask(selectedTaskJobId.value);
      } else {
        selectedTaskJobId.value = null;
        selectedTaskEvents.value = [];
      }
    }
  } catch (error) {
    if (requestId !== taskHistoryRequestId) return;
    errorMessage.value = error instanceof Error ? error.message : t('taskHistoryLoadFailed');
  } finally {
    if (requestId === taskHistoryRequestId) {
      loadingTaskHistory.value = false;
    }
  }
}

/** Read status events for an individual task. */
async function selectTask(jobId: string): Promise<void> {
  const requestId = ++taskEventsRequestId;
  selectedTaskJobId.value = jobId;
  selectedTaskEvents.value = [];
  loadingTaskEvents.value = true;

  try {
    const nextEvents = await getTaskHistoryEvents(jobId);
    if (requestId !== taskEventsRequestId || selectedTaskJobId.value !== jobId) return;

    selectedTaskEvents.value = nextEvents;
    errorMessage.value = '';
  } catch (error) {
    if (requestId !== taskEventsRequestId || selectedTaskJobId.value !== jobId) return;
    errorMessage.value = error instanceof Error ? error.message : t('taskStatusLoadFailed');
  } finally {
    if (requestId === taskEventsRequestId && selectedTaskJobId.value === jobId) {
      loadingTaskEvents.value = false;
    }
  }
}

/** Clear local task history. */
async function handleClearTaskHistory(): Promise<void> {
  if (!confirmingClearTaskHistory.value) {
    confirmingClearTaskHistory.value = true;
    errorMessage.value = '';
    successMessage.value = '';
    return;
  }

  clearingTaskHistory.value = true;
  errorMessage.value = '';
  successMessage.value = '';

  try {
    await clearTaskHistory();
    taskHistoryRequestId++;
    taskEventsRequestId++;
    taskHistory.value = [];
    selectedTaskJobId.value = null;
    selectedTaskEvents.value = [];
    confirmingClearTaskHistory.value = false;
    successMessage.value = t('taskHistoryCleared');
  } catch (error) {
    confirmingClearTaskHistory.value = false;
    errorMessage.value = error instanceof Error ? error.message : t('taskHistoryClearFailed');
  } finally {
    clearingTaskHistory.value = false;
  }
}

/** Save allowlist without committing unconfirmed changes in settings and remote panels. */
async function persistWhitelistChanges(
  previousSecurity: AgentConfig['security'],
): Promise<boolean> {
  if (!config.value) return false;
  savingWhitelist.value = true;
  errorMessage.value = '';
  successMessage.value = '';

  try {
    const persistedConfig = await getConfig();
    persistedConfig.security = {
      allowed_origins: [...config.value.security.allowed_origins],
      allowed_ips: [...config.value.security.allowed_ips],
    };
    const savedConfig = normalizeConfig(await saveConfig(persistedConfig));
    config.value.security = savedConfig.security;
    return true;
  } catch (error) {
    config.value.security = previousSecurity;
    errorMessage.value = error instanceof Error ? error.message : t('saveOrRestartFailed');
    return false;
  } finally {
    savingWhitelist.value = false;
  }
}

/** Add validated browser Origin to allowlist and save immediately. */
async function addOrigin(): Promise<void> {
  if (!config.value) return;
  const origin = originDraft.value.trim();
  originErrorMessage.value = '';

  if (!origin) return;
  if (!isValidOrigin(origin)) {
    originErrorMessage.value = t('invalidOrigin');
    return;
  }
  if (config.value.security.allowed_origins.includes(origin)) return;

  const previousSecurity = {
    allowed_origins: [...config.value.security.allowed_origins],
    allowed_ips: [...config.value.security.allowed_ips],
  };
  config.value.security.allowed_origins.push(origin);
  originDraft.value = '';
  if (!(await persistWhitelistChanges(previousSecurity))) originDraft.value = origin;
}

/** Validate that Origin string contains only scheme and host. */
function isValidOrigin(value: string): boolean {
  try {
    const url = new URL(value);
    if (url.protocol !== 'http:' && url.protocol !== 'https:') return false;

    return `${url.protocol}//${url.host}` === value;
  } catch {
    return false;
  }
}

/** Remove a browser Origin from allowlist and save immediately. */
async function removeOrigin(origin: string): Promise<void> {
  if (!config.value) return;
  const previousSecurity = {
    allowed_origins: [...config.value.security.allowed_origins],
    allowed_ips: [...config.value.security.allowed_ips],
  };
  config.value.security.allowed_origins = config.value.security.allowed_origins.filter(
    (item) => item !== origin,
  );
  await persistWhitelistChanges(previousSecurity);
}

/** Normalize IP allowlist ensuring default loopback address is always present. */
function normalizeAllowedIps(entries: string[]): string[] {
  const normalized = [REQUIRED_LOOPBACK_IP];
  for (const rawEntry of entries) {
    const entry = rawEntry.trim();
    if (!entry || entry === REQUIRED_LOOPBACK_IP) continue;
    if (!normalized.includes(entry)) normalized.push(entry);
  }

  return normalized;
}

/** Determine whether IP allowlist entry is the non-deletable default entry. */
function isFixedAllowedIp(entry: string): boolean {
  return entry === REQUIRED_LOOPBACK_IP;
}

/** Determine whether input is a prohibited wildcard/any-address entry. */
function isAnyAddressEntry(entry: string): boolean {
  return ['0.0.0.0', '::', '0.0.0.0/0', '::/0'].includes(entry);
}

/** Lightweight frontend check for IP or CIDR; backend performs authoritative validation. */
function isValidAllowedIpEntry(value: string): boolean {
  const entry = value.trim();
  if (!entry || isAnyAddressEntry(entry)) return false;

  if (entry.includes('/')) {
    const [address, prefixText, extra] = entry.split('/');
    if (!address || !prefixText || extra !== undefined) return false;
    const prefix = Number(prefixText);
    if (!Number.isInteger(prefix)) return false;

    const maxPrefix = address.includes(':') ? 128 : 32;
    if (prefix <= 0 || prefix > maxPrefix) return false;

    return isValidIpAddress(address);
  }

  return isValidIpAddress(entry);
}

/** Frontend basic IP format check to prevent obvious typos; backend handles complete IPv6/CIDR parsing. */
function isValidIpAddress(value: string): boolean {
  if (value.includes(':')) {
    return /^[0-9a-fA-F:]+$/.test(value) && value.includes(':');
  }

  const parts = value.split('.');
  return (
    parts.length === 4 &&
    parts.every((part) => {
      if (!/^\d+$/.test(part)) return false;
      const number = Number(part);
      return number >= 0 && number <= 255 && part === String(number);
    })
  );
}

/** Add validated IP or CIDR to allowlist and save immediately. */
async function addAllowedIp(): Promise<void> {
  if (!config.value) return;
  const entry = ipDraft.value.trim();
  ipErrorMessage.value = '';

  if (!entry) return;
  if (!isValidAllowedIpEntry(entry)) {
    ipErrorMessage.value = isAnyAddressEntry(entry) ? t('invalidAnyIp') : t('invalidIp');
    return;
  }
  if (config.value.security.allowed_ips.includes(entry)) return;

  const previousSecurity = {
    allowed_origins: [...config.value.security.allowed_origins],
    allowed_ips: [...config.value.security.allowed_ips],
  };
  config.value.security.allowed_ips.push(entry);
  config.value.security.allowed_ips = normalizeAllowedIps(config.value.security.allowed_ips);
  ipDraft.value = '';
  if (!(await persistWhitelistChanges(previousSecurity))) ipDraft.value = entry;
}

/** Remove a user-added entry from IP allowlist and save immediately. */
async function removeAllowedIp(entry: string): Promise<void> {
  if (!config.value) return;
  if (isFixedAllowedIp(entry)) {
    ipErrorMessage.value = t('loopbackFixed');
    return;
  }

  const previousSecurity = {
    allowed_origins: [...config.value.security.allowed_origins],
    allowed_ips: [...config.value.security.allowed_ips],
  };
  config.value.security.allowed_ips = normalizeAllowedIps(
    config.value.security.allowed_ips.filter((item) => item !== entry),
  );
  await persistWhitelistChanges(previousSecurity);
}

/** Format RFC3339 timestamp for UI display. */
function formatDateTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;

  return date.toLocaleString();
}

/** Return task status label. */
function taskStatusLabel(status: TaskHistoryStatus): string {
  const labels: Record<UiLanguage, Record<TaskHistoryStatus, string>> = {
    'zh-CN': {
      queued: '已排队',
      downloading: '下载中',
      printing: '提交中',
      submitted: '已提交系统队列',
      completed: '系统队列已完成',
      failed: '失败',
      unknown: '无法确认后续状态',
      cancelled: '已取消',
    },
    en: {
      queued: 'Queued',
      downloading: 'Downloading',
      printing: 'Submitting',
      submitted: 'Submitted to system queue',
      completed: 'System queue completed',
      failed: 'Failed',
      unknown: 'Unable to confirm next status',
      cancelled: 'Cancelled',
    },
  };

  return labels[currentLanguage.value][status];
}

/** Return task source label. */
function taskSourceLabel(source: TaskHistorySource): string {
  const labels: Record<UiLanguage, Record<TaskHistorySource, string>> = {
    'zh-CN': {
      web_socket: '网页',
      remote: '远程',
      test: '测试',
    },
    en: {
      web_socket: 'Web',
      remote: 'Remote',
      test: 'Test',
    },
  };

  return labels[currentLanguage.value][source];
}

/** Format byte count for update download progress display. */
function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  const kib = value / 1024;
  if (kib < 1024) return `${kib.toFixed(1)} KB`;

  return `${(kib / 1024).toFixed(1)} MB`;
}

/** Format millimeter dimensions for display as integer millimeters in settings. */
function formatMillimeters(value: number): string {
  return Math.round(value).toString();
}

/** Generate paper dropdown option label avoiding duplicate dimension display. */
function formatPaperLabel(paper: PaperInfo): string {
  const sizeLabel = `${formatMillimeters(paper.width_mm)} x ${formatMillimeters(paper.height_mm)} mm`;
  if (/^\d+(?:\.\d+)? x \d+(?:\.\d+)? mm$/.test(paper.name)) {
    return sizeLabel;
  }

  return `${paper.name} · ${sizeLabel}`;
}

/** Check whether desktop application updates are available. */
async function checkForUpdate(): Promise<void> {
  updateStatus.value = 'checking';
  updateMessage.value = '';
  availableUpdate.value = null;
  updateInfo.value = null;
  updateProgress.value = {
    downloadedBytes: 0,
    contentLength: null,
  };

  try {
    const update = await checkForAppUpdate();
    if (!update) {
      updateStatus.value = 'not-available';
      updateMessage.value = t('latestVersion');
      return;
    }

    availableUpdate.value = update;
    updateInfo.value = toUpdateInfo(update);
    updateStatus.value = 'available';
    updateMessage.value = `${t('updateAvailablePrefix')}${update.version}`;
  } catch (error) {
    updateStatus.value = 'error';
    updateMessage.value = error instanceof Error ? error.message : t('checkUpdateFailed');
  }
}

/** Download and install currently available update. */
async function installAvailableUpdate(): Promise<void> {
  if (!availableUpdate.value) return;

  updateStatus.value = 'downloading';
  updateMessage.value = t('downloadingInstallingUpdate');
  updateProgress.value = {
    downloadedBytes: 0,
    contentLength: null,
  };

  try {
    updateProgress.value = await downloadAndInstallAppUpdate(availableUpdate.value, (progress) => {
      updateProgress.value = progress;
    });
    updateStatus.value = 'installed';
    updateMessage.value = t('updateInstalled');
  } catch (error) {
    updateStatus.value = 'error';
    updateMessage.value = error instanceof Error ? error.message : t('installUpdateFailed');
  }
}

/** Restart application after update installation completes. */
async function restartAfterUpdate(): Promise<void> {
  try {
    await relaunchApp();
  } catch (error) {
    updateStatus.value = 'error';
    updateMessage.value = error instanceof Error ? error.message : t('restartAppFailed');
  }
}

/** Open project repository in default system browser. */
async function openGitHubRepository(): Promise<void> {
  try {
    await openUrl(GITHUB_REPOSITORY_URL);
  } catch (error) {
    updateStatus.value = 'error';
    updateMessage.value = error instanceof Error ? error.message : t('openGithubFailed');
  }
}

/** Open GitHub Releases page in default system browser. */
async function openReleaseNotes(): Promise<void> {
  try {
    await openUrl(GITHUB_RELEASES_URL);
  } catch (error) {
    updateStatus.value = 'error';
    updateMessage.value = error instanceof Error ? error.message : t('openReleaseNotesFailed');
  }
}

async function loadCliIntegrationStatus(): Promise<void> {
  try {
    cliIntegrationStatus.value = await getCliIntegrationStatus();
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : t('cliOperationFailed');
  }
}

async function changeCliIntegration(action: 'install' | 'uninstall'): Promise<void> {
  changingCliIntegration.value = true;
  errorMessage.value = '';
  successMessage.value = '';
  try {
    cliIntegrationStatus.value =
      action === 'install' ? await installCliIntegration() : await uninstallCliIntegration();
    successMessage.value = t(action === 'install' ? 'cliInstalled' : 'cliUninstalled');
  } catch (error) {
    errorMessage.value = error instanceof Error ? error.message : t('cliOperationFailed');
  } finally {
    changingCliIntegration.value = false;
  }
}

applyTheme(themeMode.value);

onMounted(() => {
  // Synchronize theme listeners first before loading asynchronous application data on initial mount.
  setupThemeSync();
  void loadConfig();
  void loadAppVersion();
  void loadCliIntegrationStatus();
});

onBeforeUnmount(() => {
  // Remove global media query listeners when settings page unmounts.
  colorSchemeQuery?.removeEventListener('change', handleSystemThemeChange);
});
</script>

<template>
  <main class="min-h-screen bg-muted/30 px-4 py-4 text-foreground md:px-6">
    <div class="mx-auto flex w-full max-w-5xl flex-col gap-4">
      <header
        class="flex flex-col gap-3 border-b bg-background/80 pb-4 md:flex-row md:items-center md:justify-between"
      >
        <div data-tour="app-status">
          <h1 class="text-xl font-semibold tracking-normal">PrintBridge</h1>
          <p class="text-sm text-muted-foreground">{{ t('localPort') }} {{ servicePort || '-' }}</p>
        </div>
        <div data-tour="config-transfer" class="flex flex-wrap items-center gap-2">
          <button
            type="button"
            data-testid="status-doctor-trigger"
            class="rounded-full focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:outline-none"
            :aria-label="t('doctor.open')"
            @click="doctorOpen = true"
          >
            <Badge :variant="statusVariant">
              {{ statusLabel }}
            </Badge>
          </button>
          <Button v-if="showsSaveButton" :disabled="!config || saving" @click="persistConfig">
            <Save class="size-4" />
            {{ saving ? t('saving') : t('save') }}
          </Button>
          <Button
            variant="outline"
            :disabled="!config || exportingConfig"
            @click="openExportDialog"
          >
            <FileDown class="size-4" />
            {{ t('exportConfig') }}
          </Button>
          <Button
            variant="outline"
            :disabled="!config || importingConfig"
            @click="openImportDialog"
          >
            <FileUp class="size-4" />
            {{ t('importConfig') }}
          </Button>
        </div>
      </header>

      <div v-if="errorMessage || successMessage" class="grid gap-2 text-sm">
        <Alert
          v-if="errorMessage"
          variant="error"
          class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3"
        >
          <AlertDescription class="min-w-0 break-words">
            {{ errorMessage }}
          </AlertDescription>
          <Button
            variant="ghost"
            size="icon-sm"
            class="shrink-0 text-destructive hover:bg-destructive/15 hover:text-destructive"
            :aria-label="t('closeError')"
            @click="dismissMessage('error')"
          >
            <X class="size-4" />
          </Button>
        </Alert>
        <Alert
          v-if="successMessage"
          variant="success"
          class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3"
        >
          <AlertDescription class="min-w-0 break-words">
            {{ successMessage }}
          </AlertDescription>
          <Button
            variant="ghost"
            size="icon-sm"
            class="shrink-0 text-emerald-800 hover:bg-emerald-500/15 hover:text-emerald-900 dark:text-emerald-200 dark:hover:text-emerald-100"
            :aria-label="t('closeSuccess')"
            @click="dismissMessage('success')"
          >
            <X class="size-4" />
          </Button>
        </Alert>
      </div>

      <Card v-if="loadingConfig">
        <CardContent class="py-12 text-center text-sm text-muted-foreground">
          {{ t('loadingSettings') }}
        </CardContent>
      </Card>

      <Tabs v-else-if="config" v-model="activeTab">
        <TabsList class="grid w-full grid-cols-6 md:w-[720px]">
          <TabsTrigger value="settings"> {{ t('settings') }} </TabsTrigger>
          <TabsTrigger value="remote"> {{ t('remote') }} </TabsTrigger>
          <TabsTrigger value="website-whitelist"> {{ t('websiteWhitelist') }} </TabsTrigger>
          <TabsTrigger value="ip-whitelist"> {{ t('ipWhitelist') }} </TabsTrigger>
          <TabsTrigger value="logs"> {{ t('tasks') }} </TabsTrigger>
          <TabsTrigger value="updates"> {{ t('about') }} </TabsTrigger>
        </TabsList>

        <TabsContent value="settings" class="mt-2">
          <Card>
            <CardContent class="grid gap-3 px-4 pt-2 pb-4">
              <div data-tour="print-settings" class="grid gap-3">
                <div class="grid gap-2 md:grid-cols-[minmax(0,1fr)_auto_auto] md:items-end">
                  <div class="grid gap-2">
                    <Label for="default-printer">{{ t('defaultPrinter') }}</Label>
                    <Select
                      :model-value="selectedPrinter"
                      @update:model-value="handlePrinterChange(String($event))"
                    >
                      <SelectTrigger id="default-printer" class="w-full">
                        <SelectValue :placeholder="t('selectPrinter')" />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem
                          v-for="printer in printers"
                          :key="printer.name"
                          :value="printer.name"
                        >
                          {{ printer.name
                          }}{{ printer.is_default ? ` (${t('systemDefault')})` : '' }}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <Button
                    class="whitespace-nowrap"
                    variant="outline"
                    :disabled="loadingPrinters"
                    @click="refreshPrinters"
                  >
                    <RefreshCw class="size-4" :class="{ 'animate-spin': loadingPrinters }" />
                    {{ t('refresh') }}
                  </Button>
                  <Button
                    class="whitespace-nowrap"
                    variant="outline"
                    :disabled="!canTestPrint || testingPrint"
                    @click="handleTestPrint"
                  >
                    <Printer class="size-4" />
                    {{ testingPrint ? t('submitting') : t('testPrint') }}
                  </Button>
                </div>

                <div
                  class="grid items-start gap-4 md:grid-cols-[minmax(0,1.35fr)_minmax(140px,0.45fr)_minmax(140px,0.45fr)]"
                >
                  <div class="grid gap-2">
                    <Label for="default-paper">{{ t('defaultPaper') }}</Label>
                    <Select
                      :model-value="selectedPaper"
                      @update:model-value="selectedPaper = String($event)"
                    >
                      <SelectTrigger id="default-paper" class="w-full">
                        <SelectValue :placeholder="t('selectPaper')" />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="custom"> {{ t('customSize') }} </SelectItem>
                        <SelectItem v-for="paper in papers" :key="paper.id" :value="paper.id">
                          {{ formatPaperLabel(paper) }}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                    <p class="text-xs text-muted-foreground">
                      {{
                        loadingPapers
                          ? t('loadingPapers')
                          : papers.length
                            ? t('paperListReady')
                            : t('paperListEmpty')
                      }}
                    </p>
                  </div>
                  <div class="grid gap-2">
                    <Label for="paper-width">{{ t('widthMm') }}</Label>
                    <Input
                      id="paper-width"
                      type="number"
                      min="1"
                      step="0.1"
                      :model-value="
                        config.printing.default_paper?.width_mm ?? DEFAULT_PAPER.width_mm
                      "
                      @update:model-value="setPaperDimension('width_mm', $event)"
                    />
                  </div>
                  <div class="grid gap-2">
                    <Label for="paper-height">{{ t('heightMm') }}</Label>
                    <Input
                      id="paper-height"
                      type="number"
                      min="1"
                      step="0.1"
                      :model-value="
                        config.printing.default_paper?.height_mm ?? DEFAULT_PAPER.height_mm
                      "
                      @update:model-value="setPaperDimension('height_mm', $event)"
                    />
                  </div>
                </div>
              </div>

              <Separator />

              <div data-tour="service-settings" class="grid gap-4 md:grid-cols-2">
                <div class="grid gap-2">
                  <Label for="service-port">{{ t('localPort') }}</Label>
                  <Input
                    id="service-port"
                    type="number"
                    :min="MIN_SERVICE_PORT"
                    :max="MAX_SERVICE_PORT"
                    :model-value="config.service.port"
                    @update:model-value="setPort"
                  />
                  <p v-if="hasPendingPortChange" class="text-xs text-muted-foreground">
                    {{ t('portPendingPrefix') }} {{ activePort }}{{ t('portPendingSuffix') }}
                  </p>
                </div>
                <div class="grid gap-2">
                  <Label for="autostart">{{ t('autostart') }}</Label>
                  <div class="flex min-h-10 items-center justify-between gap-3">
                    <p class="text-xs text-muted-foreground">{{ t('autostartHint') }}</p>
                    <Switch id="autostart" v-model="config.app.autostart" />
                  </div>
                </div>
              </div>

              <Separator />

              <div data-tour="preferences-cli" class="grid gap-3">
                <div class="grid gap-3 rounded-md border px-3 py-2 md:grid-cols-2">
                  <div class="grid gap-2">
                    <Label>{{ t('appearance') }}</Label>
                    <div
                      class="grid w-full grid-cols-3 gap-1 rounded-lg border bg-muted/40 p-1"
                      role="group"
                      :aria-label="t('appearance')"
                    >
                      <button
                        type="button"
                        :class="themeOptionClass('light')"
                        :aria-pressed="themeMode === 'light'"
                        @click="setThemeMode('light')"
                      >
                        <Sun class="size-4" />
                        <span>{{ t('light') }}</span>
                      </button>
                      <button
                        type="button"
                        :class="themeOptionClass('dark')"
                        :aria-pressed="themeMode === 'dark'"
                        @click="setThemeMode('dark')"
                      >
                        <Moon class="size-4" />
                        <span>{{ t('dark') }}</span>
                      </button>
                      <button
                        type="button"
                        :class="themeOptionClass('system')"
                        :aria-pressed="themeMode === 'system'"
                        @click="setThemeMode('system')"
                      >
                        <Monitor class="size-4" />
                        <span>{{ t('system') }}</span>
                      </button>
                    </div>
                  </div>
                  <div class="grid gap-2">
                    <Label>{{ t('language') }}</Label>
                    <div
                      class="grid w-full grid-cols-2 gap-1 rounded-lg border bg-muted/40 p-1"
                      role="group"
                      :aria-label="t('language')"
                    >
                      <button
                        type="button"
                        :class="languageOptionClass('zh-CN')"
                        :aria-pressed="currentLanguage === 'zh-CN'"
                        @click="setLanguage('zh-CN')"
                      >
                        <span>{{ t('chinese') }}</span>
                      </button>
                      <button
                        type="button"
                        :class="languageOptionClass('en')"
                        :aria-pressed="currentLanguage === 'en'"
                        @click="setLanguage('en')"
                      >
                        <span>{{ t('english') }}</span>
                      </button>
                    </div>
                  </div>
                </div>

                <CliIntegrationRow
                  v-if="cliIntegrationStatus"
                  :status="cliIntegrationStatus"
                  :loading="changingCliIntegration"
                  @install="changeCliIntegration('install')"
                  @uninstall="changeCliIntegration('uninstall')"
                />
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="remote" class="mt-2">
          <Card>
            <CardContent data-tour="remote-settings" class="grid gap-5 pt-6">
              <div class="flex items-center justify-between gap-3">
                <Label for="remote-enabled">{{ t('enableRemoteTasks') }}</Label>
                <Switch id="remote-enabled" v-model="config.remote.enabled" />
              </div>
              <div class="grid gap-2 md:grid-cols-[minmax(0,1fr)_auto] md:items-end">
                <div class="grid min-w-0 gap-2">
                  <Label for="remote-url">{{ t('taskUrl') }}</Label>
                  <Input
                    id="remote-url"
                    type="url"
                    placeholder="https://api.example.com/print-task"
                    :model-value="config.remote.endpoint_url ?? ''"
                    @update:model-value="setRemoteString('endpoint_url', $event)"
                  />
                </div>
                <Button
                  class="whitespace-nowrap"
                  variant="outline"
                  :disabled="!config.remote.enabled || testingRemote"
                  @click="handleTestRemoteConnection"
                >
                  <RefreshCw class="size-4" :class="{ 'animate-spin': testingRemote }" />
                  {{ testingRemote ? t('testing') : t('testConnection') }}
                </Button>
              </div>

              <div class="grid gap-2">
                <Label for="remote-token">Authorization Bearer Token</Label>
                <Input
                  id="remote-token"
                  type="password"
                  autocomplete="off"
                  :model-value="config.remote.bearer_token ?? ''"
                  @update:model-value="setRemoteString('bearer_token', $event)"
                />
              </div>

              <div class="grid gap-4 md:grid-cols-2">
                <div class="grid gap-2">
                  <Label for="remote-device-id">Device ID</Label>
                  <div class="flex gap-2">
                    <Input
                      id="remote-device-id"
                      class="min-w-0 flex-1"
                      :model-value="config.remote.device_id ?? ''"
                      @update:model-value="setRemoteString('device_id', $event)"
                    />
                    <Button
                      class="whitespace-nowrap"
                      variant="outline"
                      type="button"
                      @click="generateRemoteDeviceId"
                    >
                      <Shuffle class="size-4" />
                      {{ t('randomGenerate') }}
                    </Button>
                  </div>
                </div>
                <div class="grid gap-2">
                  <Label for="remote-device-name">Device Name</Label>
                  <Input
                    id="remote-device-name"
                    :model-value="config.remote.device_name ?? ''"
                    @update:model-value="setRemoteString('device_name', $event)"
                  />
                </div>
              </div>

              <div class="grid gap-4 md:grid-cols-2">
                <div class="grid gap-2">
                  <Label for="remote-poll-interval">{{ t('pollIntervalSeconds') }}</Label>
                  <Input
                    id="remote-poll-interval"
                    type="number"
                    :min="MIN_REMOTE_POLL_INTERVAL_SECONDS"
                    :model-value="config.remote.poll_interval_seconds"
                    @update:model-value="setRemoteNumber('poll_interval_seconds', $event)"
                  />
                </div>
                <div class="grid gap-2">
                  <Label for="remote-max-retries">{{ t('reportRetries') }}</Label>
                  <Input
                    id="remote-max-retries"
                    type="number"
                    :min="MIN_REMOTE_MAX_REPORT_RETRIES"
                    :model-value="config.remote.max_report_retries"
                    @update:model-value="setRemoteNumber('max_report_retries', $event)"
                  />
                </div>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="website-whitelist" class="mt-2">
          <Card>
            <CardContent data-tour="website-whitelist" class="grid gap-4 pt-6">
              <form class="flex items-start gap-2" @submit.prevent="addOrigin">
                <div class="grid flex-1 gap-1">
                  <Input
                    v-model="originDraft"
                    placeholder="https://example.com"
                    autocomplete="off"
                    :aria-invalid="originErrorMessage ? 'true' : 'false'"
                  />
                  <p v-if="originErrorMessage" class="text-xs text-destructive">
                    {{ originErrorMessage }}
                  </p>
                </div>
                <Button type="submit" :disabled="savingWhitelist"> {{ t('add') }} </Button>
              </form>
              <div class="grid gap-2">
                <div
                  v-for="origin in config.security.allowed_origins"
                  :key="origin"
                  class="flex items-center justify-between rounded-md border px-3 py-2 text-sm"
                >
                  <span class="truncate">{{ origin }}</span>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    :aria-label="t('deleteOrigin')"
                    :disabled="savingWhitelist"
                    @click="removeOrigin(origin)"
                  >
                    <Trash2 class="size-4" />
                  </Button>
                </div>
                <p
                  v-if="config.security.allowed_origins.length === 0"
                  class="rounded-md border border-dashed px-3 py-6 text-center text-sm text-muted-foreground"
                >
                  {{ t('noWhitelistOrigins') }}
                </p>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="ip-whitelist" class="mt-2">
          <Card>
            <CardContent data-tour="ip-whitelist" class="grid gap-4 pt-6">
              <form class="flex items-start gap-2" @submit.prevent="addAllowedIp">
                <div class="grid flex-1 gap-1">
                  <Input
                    v-model="ipDraft"
                    :placeholder="t('ipPlaceholder')"
                    autocomplete="off"
                    :aria-invalid="ipErrorMessage ? 'true' : 'false'"
                  />
                  <p v-if="ipErrorMessage" class="text-xs text-destructive">
                    {{ ipErrorMessage }}
                  </p>
                </div>
                <Button type="submit" :disabled="savingWhitelist"> {{ t('add') }} </Button>
              </form>
              <div class="grid gap-2">
                <div
                  v-for="entry in config.security.allowed_ips"
                  :key="entry"
                  class="flex items-center justify-between gap-3 rounded-md border px-3 py-2 text-sm"
                >
                  <span class="truncate">{{ entry }}</span>
                  <Button
                    v-if="!isFixedAllowedIp(entry)"
                    variant="ghost"
                    size="icon-sm"
                    :aria-label="t('deleteIpWhitelist')"
                    :disabled="savingWhitelist"
                    @click="removeAllowedIp(entry)"
                  >
                    <Trash2 class="size-4" />
                  </Button>
                  <Badge v-else variant="secondary"> {{ t('default') }} </Badge>
                </div>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="updates" class="mt-2">
          <div data-tour="about">
            <Card>
              <CardContent class="grid gap-5 p-5">
                <div class="grid gap-4 md:grid-cols-[minmax(0,1fr)_auto] md:items-start">
                  <div class="flex min-w-0 items-start gap-3">
                    <img :src="appIcon" alt="" class="mt-1 size-10 rounded-md" />
                    <div class="min-w-0">
                      <div class="flex flex-wrap items-center gap-2">
                        <h3 class="text-xl font-semibold tracking-normal">PrintBridge</h3>
                        <Badge variant="outline">{{ t('version') }} v{{ currentAppVersion }}</Badge>
                      </div>
                      <p class="mt-2 text-sm text-muted-foreground">{{ t('aboutDescription') }}</p>
                    </div>
                  </div>

                  <div class="flex flex-wrap gap-2 md:shrink-0 md:flex-nowrap md:justify-end">
                    <Button variant="outline" @click="replayOnboarding">
                      <CircleHelp class="size-4" />
                      {{ t('replayOnboarding') }}
                    </Button>
                    <Button variant="outline" @click="openGitHubRepository">
                      <ExternalLink class="size-4" />
                      GitHub
                    </Button>
                    <Button variant="outline" @click="openReleaseNotes">
                      <ExternalLink class="size-4" />
                      {{ t('releaseNotes') }}
                    </Button>
                    <Button
                      variant="outline"
                      :disabled="checkingUpdate || installingUpdate"
                      @click="checkForUpdate"
                    >
                      <RefreshCw class="size-4" :class="{ 'animate-spin': checkingUpdate }" />
                      {{ checkingUpdate ? t('checking') : t('checkUpdates') }}
                    </Button>
                    <Button
                      v-if="availableUpdate && updateStatus !== 'installed'"
                      :disabled="installingUpdate"
                      @click="installAvailableUpdate"
                    >
                      <Download class="size-4" />
                      {{ updateButtonLabel }}
                    </Button>
                    <Button
                      v-if="updateStatus === 'installed'"
                      variant="outline"
                      @click="restartAfterUpdate"
                    >
                      <RotateCw class="size-4" />
                      {{ t('restartApp') }}
                    </Button>
                  </div>
                </div>

                <div
                  class="rounded-md border px-4 py-3 text-sm"
                  :class="{
                    'border-primary/40 bg-primary/10 text-primary': updateStatus === 'available',
                    'border-destructive/30 bg-destructive/10 text-destructive':
                      updateStatus === 'error',
                    'bg-muted/40 text-muted-foreground':
                      updateStatus !== 'available' && updateStatus !== 'error',
                  }"
                >
                  <span v-if="updateStatus === 'available'">
                    {{ t('updateAvailablePrefix') }}{{ availableUpdateVersion }}
                  </span>
                  <span v-else-if="updateMessage">{{ updateMessage }}</span>
                  <span v-else>{{ t('updateStatusIdle') }}</span>
                </div>

                <div v-if="installingUpdate" class="grid gap-2">
                  <div class="h-2 overflow-hidden rounded-full bg-muted">
                    <div
                      class="h-full rounded-full bg-primary transition-all"
                      :style="{ width: `${updateProgressPercent}%` }"
                    />
                  </div>
                  <p class="text-xs text-muted-foreground">
                    {{ updateProgressText }}
                  </p>
                </div>

                <div v-if="updateInfo?.body" class="grid gap-2">
                  <Label>{{ t('updateNotes') }}</Label>
                  <div
                    class="max-h-36 overflow-auto whitespace-pre-wrap rounded-md border bg-muted/20 px-3 py-2 text-sm"
                  >
                    {{ updateInfo.body }}
                  </div>
                </div>
              </CardContent>
            </Card>
          </div>
        </TabsContent>

        <TabsContent value="logs" class="mt-2">
          <Card data-tour="task-history">
            <CardHeader class="flex flex-row items-center justify-between pb-3">
              <CardTitle class="text-base"> {{ t('printTasks') }} </CardTitle>
              <div class="flex items-center gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  :disabled="loadingTaskHistory || clearingTaskHistory"
                  @click="refreshTaskHistory"
                >
                  <RefreshCw class="size-4" :class="{ 'animate-spin': loadingTaskHistory }" />
                  {{ t('refresh') }}
                </Button>
                <Button
                  :variant="confirmingClearTaskHistory ? 'destructive' : 'outline'"
                  size="sm"
                  :disabled="loadingTaskHistory || clearingTaskHistory || taskHistory.length === 0"
                  @click="handleClearTaskHistory"
                >
                  <Trash2 class="size-4" />
                  {{ confirmingClearTaskHistory ? t('confirmClear') : t('clear') }}
                </Button>
              </div>
            </CardHeader>
            <CardContent class="grid gap-4 xl:grid-cols-[minmax(0,1.45fr)_minmax(320px,0.8fr)]">
              <ScrollArea class="h-[420px] rounded-md border">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead class="w-[170px]"> {{ t('time') }} </TableHead>
                      <TableHead class="w-[145px]"> {{ t('status') }} </TableHead>
                      <TableHead class="w-[80px]"> {{ t('source') }} </TableHead>
                      <TableHead class="w-[150px]"> Job </TableHead>
                      <TableHead class="w-[150px]"> {{ t('printer') }} </TableHead>
                      <TableHead> {{ t('message') }} </TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    <TableEmpty v-if="taskHistory.length === 0" :colspan="6">
                      {{ loadingTaskHistory ? t('loadingTasks') : t('noTasks') }}
                    </TableEmpty>
                    <TableRow
                      v-for="entry in taskHistory"
                      v-else
                      :key="entry.job_id"
                      class="cursor-pointer"
                      :class="{ 'bg-muted/60': selectedTaskJobId === entry.job_id }"
                      @click="selectTask(entry.job_id)"
                    >
                      <TableCell class="whitespace-nowrap text-muted-foreground">
                        {{ formatDateTime(entry.updated_at) }}
                      </TableCell>
                      <TableCell>
                        <Badge variant="outline">
                          {{ taskStatusLabel(entry.current_status) }}
                        </Badge>
                      </TableCell>
                      <TableCell class="whitespace-nowrap text-muted-foreground">
                        {{ taskSourceLabel(entry.source) }}
                      </TableCell>
                      <TableCell class="max-w-[150px] truncate font-mono text-xs">
                        {{ entry.job_id }}
                      </TableCell>
                      <TableCell class="max-w-[150px] truncate text-muted-foreground">
                        {{ entry.printer_name ?? '-' }}
                      </TableCell>
                      <TableCell class="min-w-[180px] whitespace-normal wrap-anywhere">
                        {{ entry.current_message ?? '-' }}
                      </TableCell>
                    </TableRow>
                  </TableBody>
                </Table>
              </ScrollArea>

              <div class="min-w-0 rounded-md border">
                <div class="min-w-0 border-b px-3 py-2">
                  <div class="truncate text-sm font-medium">
                    {{ selectedTask?.job_id ?? t('taskDetails') }}
                  </div>
                  <div class="mt-1 break-words text-xs text-muted-foreground">
                    {{
                      selectedTask
                        ? `${selectedTask.paper_name ?? '-'} · ${selectedTask.copies ?? '-'} ${t('copiesUnit')}`
                        : t('selectTaskHint')
                    }}
                  </div>
                </div>
                <ScrollArea class="h-[360px]">
                  <div v-if="!selectedTaskJobId" class="px-3 py-8 text-sm text-muted-foreground">
                    {{ t('noSelectedTask') }}
                  </div>
                  <div
                    v-else-if="loadingTaskEvents"
                    class="px-3 py-8 text-sm text-muted-foreground"
                  >
                    {{ t('loadingStatus') }}
                  </div>
                  <div
                    v-else-if="selectedTaskEvents.length === 0"
                    class="px-3 py-8 text-sm text-muted-foreground"
                  >
                    {{ t('noStatusRecords') }}
                  </div>
                  <div v-else class="grid gap-3 p-3">
                    <div
                      v-for="event in selectedTaskEvents"
                      :key="event.id"
                      class="grid min-w-0 gap-1 border-l-2 border-muted-foreground/30 pl-3"
                    >
                      <div class="flex flex-wrap items-center gap-2">
                        <Badge variant="outline">
                          {{ taskStatusLabel(event.status) }}
                        </Badge>
                        <span class="text-xs text-muted-foreground">
                          {{ formatDateTime(event.occurred_at) }}
                        </span>
                      </div>
                      <div class="break-words text-sm">
                        {{ event.message ?? '-' }}
                      </div>
                    </div>
                  </div>
                </ScrollArea>
              </div>
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>

      <div
        v-if="showExportDialog"
        class="fixed inset-0 z-50 grid place-items-center bg-background/80 p-4 backdrop-blur-sm"
      >
        <Card class="w-full max-w-lg">
          <CardHeader class="pb-3">
            <CardTitle class="text-base">{{ t('exportConfig') }}</CardTitle>
          </CardHeader>
          <CardContent class="grid gap-4">
            <div class="grid gap-3">
              <label class="flex items-center gap-2 text-sm">
                <input v-model="exportOptions.service_port" type="checkbox" />
                {{ t('localPort') }}
              </label>
              <label class="flex items-center gap-2 text-sm">
                <input v-model="exportOptions.allowed_origins" type="checkbox" />
                {{ t('websiteWhitelist') }}
              </label>
              <label class="flex items-center gap-2 text-sm">
                <input v-model="exportOptions.allowed_ips" type="checkbox" />
                {{ t('ipWhitelist') }}
              </label>
              <label class="flex items-center gap-2 text-sm">
                <input v-model="exportOptions.remote_enabled" type="checkbox" />
                {{ t('remoteTaskSwitch') }}
              </label>
              <label class="flex items-center gap-2 text-sm">
                <input v-model="exportOptions.remote_endpoint_url" type="checkbox" />
                {{ t('remoteTaskUrl') }}
              </label>
              <label class="flex items-center gap-2 text-sm">
                <input v-model="exportOptions.remote_bearer_token" type="checkbox" />
                {{ t('remoteTaskAuthorizationToken') }}
              </label>
              <label class="flex items-center gap-2 text-sm">
                <input v-model="exportOptions.remote_poll_interval_seconds" type="checkbox" />
                {{ t('pollInterval') }}
              </label>
              <label class="flex items-center gap-2 text-sm">
                <input v-model="exportOptions.remote_max_report_retries" type="checkbox" />
                {{ t('reportRetries') }}
              </label>
            </div>

            <div class="grid gap-2">
              <Label for="export-password">{{ t('password') }}</Label>
              <Input id="export-password" v-model="exportPassword" type="password" />
            </div>

            <Alert v-if="showEmptyPasswordTokenConfirm" variant="warning">
              <AlertDescription>
                {{ t('exportEmptyPasswordWarning') }}
              </AlertDescription>
            </Alert>

            <div class="flex flex-wrap justify-end gap-2">
              <Button
                variant="outline"
                :disabled="exportingConfig"
                @click="showExportDialog = false"
              >
                {{ t('cancel') }}
              </Button>
              <Button
                v-if="showEmptyPasswordTokenConfirm"
                variant="outline"
                :disabled="exportingConfig"
                @click="confirmEmptyPasswordExport"
              >
                {{ t('confirmEmptyPasswordExport') }}
              </Button>
              <Button :disabled="exportingConfig" @click="handleExportConfig">
                {{ exportingConfig ? t('exporting') : t('export') }}
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>

      <div
        v-if="showImportDialog"
        class="fixed inset-0 z-50 grid place-items-center bg-background/80 p-4 backdrop-blur-sm"
      >
        <div class="flex max-h-full w-full items-center justify-center">
          <Card class="flex max-h-[calc(100vh-2rem)] w-full max-w-xl flex-col">
            <CardHeader class="shrink-0 pb-3">
              <CardTitle class="text-base">{{ t('importConfig') }}</CardTitle>
            </CardHeader>
            <CardContent class="flex min-h-0 flex-1 flex-col gap-4">
              <div class="grid gap-2">
                <Label for="import-path">{{ t('configFile') }}</Label>
                <div class="flex gap-2">
                  <Input
                    id="import-path"
                    class="min-w-0 flex-1"
                    :model-value="importPath"
                    readonly
                  />
                  <Button
                    variant="outline"
                    :disabled="previewingConfigImport"
                    @click="chooseImportFile"
                  >
                    {{ t('choose') }}
                  </Button>
                </div>
              </div>

              <div class="grid gap-2">
                <Label for="import-password">{{ t('password') }}</Label>
                <Input
                  id="import-password"
                  v-model="importPassword"
                  type="password"
                  @update:model-value="resetImportPreview"
                />
              </div>

              <Alert v-if="importErrorMessage" variant="error">
                <AlertDescription class="min-w-0 break-words">
                  {{ importErrorMessage }}
                </AlertDescription>
              </Alert>

              <div
                v-if="importPreview"
                class="flex min-h-0 flex-col gap-2 rounded-md border bg-muted/20 p-3"
              >
                <div class="shrink-0 text-sm font-medium">{{ t('importChanges') }}</div>
                <div v-if="importPreview.items.length === 0" class="text-sm text-muted-foreground">
                  {{ t('noImportChanges') }}
                </div>
                <div v-else class="grid max-h-[35vh] min-h-0 gap-2 overflow-y-auto pr-1">
                  <div
                    v-for="item in importPreview.items"
                    :key="item.key"
                    class="grid gap-1 text-sm"
                  >
                    <div class="font-medium">{{ item.label }}</div>
                    <div class="break-words text-muted-foreground">
                      {{ item.current }} -> {{ item.next }}
                    </div>
                  </div>
                </div>
              </div>

              <div class="flex shrink-0 flex-wrap justify-end gap-2">
                <Button
                  variant="outline"
                  :disabled="previewingConfigImport || importingConfig"
                  @click="showImportDialog = false"
                >
                  {{ t('cancel') }}
                </Button>
                <Button
                  variant="outline"
                  :disabled="!importPath || previewingConfigImport || importingConfig"
                  @click="handlePreviewConfigImport"
                >
                  {{ previewingConfigImport ? t('previewing') : t('preview') }}
                </Button>
                <Button
                  :disabled="
                    !importPreview ||
                    importPreview.items.length === 0 ||
                    previewingConfigImport ||
                    importingConfig
                  "
                  @click="handleImportConfig"
                >
                  {{ importingConfig ? t('importing') : t('confirmImport') }}
                </Button>
              </div>
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
    <StatusDoctorSheet v-model:open="doctorOpen" @status-change="doctorNeedsAttention = $event" />
  </main>
</template>
