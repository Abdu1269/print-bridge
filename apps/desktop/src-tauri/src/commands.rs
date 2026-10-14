use crate::{
    cli_integration::{self, CliIntegrationStatus},
    config::{AgentConfig, MAX_SERVICE_PORT, MIN_SERVICE_PORT},
    logs::TaskLogEntry,
    state::AgentState,
    task_history::{TaskHistoryEvent, TaskHistoryJob},
    tray::apply_tray_language,
};
use print_bridge_cli::{
    config_transfer::{ExportConfigOptions, ImportPreview},
    Command, CommandError, CommandResult, CommandService, DoctorReport, ProductKind,
};
use print_bridge_core::printing::{PaperInfo, PrinterInfo};
use std::{net::TcpListener, path::PathBuf, sync::Arc};
use tauri::State;
use tauri_plugin_autostart::ManagerExt;

/// Return current Agent configuration to Tauri frontend.
#[tauri::command]
pub async fn get_config(
    service: State<'_, Arc<CommandService>>,
) -> Result<AgentConfig, CommandError> {
    match service.execute(Command::GetConfig).await? {
        CommandResult::Config(config) => Ok(*config),
        _ => unreachable!("GetConfig returned an unexpected result"),
    }
}

/// Run read-only local environment diagnostics for Desktop product.
#[tauri::command]
pub async fn run_doctor(
    service: State<'_, Arc<CommandService>>,
) -> Result<DoctorReport, CommandError> {
    run_doctor_with_service(service.inner().as_ref()).await
}

/// Run Desktop Doctor via shared command service, allowing tests to bypass Tauri State.
async fn run_doctor_with_service(service: &CommandService) -> Result<DoctorReport, CommandError> {
    match service
        .execute(Command::Doctor {
            product: ProductKind::Desktop,
        })
        .await?
    {
        CommandResult::Doctor(report) => Ok(report),
        _ => unreachable!("Doctor returned an unexpected result"),
    }
}

/// Check whether currently running desktop application is a debug build.
#[tauri::command]
pub fn is_debug_build() -> bool {
    cfg!(debug_assertions)
}

/// Return command-line tool installation status for current platform.
#[tauri::command]
pub async fn get_cli_integration_status() -> Result<CliIntegrationStatus, CommandError> {
    run_cli_integration(cli_integration::status).await
}

/// Install command-line tool entry point manageable by current user.
#[tauri::command]
pub async fn install_cli_integration() -> Result<CliIntegrationStatus, CommandError> {
    run_cli_integration(cli_integration::install).await
}

/// Remove command-line tool entry point manageable by current user.
#[tauri::command]
pub async fn uninstall_cli_integration() -> Result<CliIntegrationStatus, CommandError> {
    run_cli_integration(cli_integration::uninstall).await
}

async fn run_cli_integration(
    operation: fn() -> Result<CliIntegrationStatus, String>,
) -> Result<CliIntegrationStatus, CommandError> {
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| {
            CommandError::new(
                print_bridge_cli::CommandErrorKind::Runtime,
                error.to_string(),
            )
        })?
        .map_err(|error| CommandError::new(print_bridge_cli::CommandErrorKind::Runtime, error))
}

/// Apply application-level settings and save complete Agent configuration.
#[tauri::command]
pub async fn save_config(
    app: tauri::AppHandle,
    config: AgentConfig,
    state: State<'_, AgentState>,
    service: State<'_, Arc<CommandService>>,
) -> Result<AgentConfig, CommandError> {
    let current = state.config.read().await.clone();
    let current_port = current.service.port;
    if config.service.port != current_port {
        check_service_port_available_for_host("0.0.0.0", config.service.port).map_err(
            |message| CommandError::new(print_bridge_cli::CommandErrorKind::InvalidInput, message),
        )?;
    }

    apply_autostart(&app, config.app.autostart).map_err(|message| {
        CommandError::new(print_bridge_cli::CommandErrorKind::Runtime, message)
    })?;

    let saved = match service.execute(Command::SaveConfig(config)).await {
        Ok(CommandResult::Config(config)) => *config,
        Ok(_) => unreachable!("SaveConfig returned an unexpected result"),
        Err(error) => {
            if let Err(rollback_error) = apply_autostart(&app, current.app.autostart) {
                return Err(CommandError::new(
                    print_bridge_cli::CommandErrorKind::Runtime,
                    format!(
                        "{error}; failed to restore the previous autostart setting: {rollback_error}"
                    ),
                ));
            }
            return Err(error);
        }
    };

    if let Err(error) = apply_tray_language(&app, saved.app.language) {
        tauri_plugin_log::log::warn!("failed to sync tray language after save: {error}");
    }

    Ok(saved)
}

/// Export current configuration to an encrypted file.
#[tauri::command]
pub async fn export_config_file(
    path: String,
    password: String,
    options: ExportConfigOptions,
    service: State<'_, Arc<CommandService>>,
) -> Result<(), CommandError> {
    service
        .execute(Command::ExportConfig {
            path: PathBuf::from(path),
            password,
            options,
        })
        .await?;
    Ok(())
}

/// Decrypt configuration file and return pre-import diff preview.
#[tauri::command]
pub async fn preview_config_import(
    path: String,
    password: String,
    service: State<'_, Arc<CommandService>>,
) -> Result<ImportPreview, CommandError> {
    match service
        .execute(Command::PreviewConfigImport {
            path: PathBuf::from(path),
            password,
        })
        .await?
    {
        CommandResult::ImportPreview(preview) => Ok(preview),
        _ => unreachable!("PreviewConfigImport returned an unexpected result"),
    }
}

/// Decrypt and import configuration file, requiring file hash to match preview.
#[tauri::command]
pub async fn import_config_file(
    path: String,
    password: String,
    expected_file_hash: String,
    service: State<'_, Arc<CommandService>>,
) -> Result<AgentConfig, CommandError> {
    match service
        .execute(Command::ImportConfig {
            path: PathBuf::from(path),
            password,
            expected_file_hash,
        })
        .await?
    {
        CommandResult::Config(config) => Ok(*config),
        _ => unreachable!("ImportConfig returned an unexpected result"),
    }
}

/// Run GET/POST connection test using remote configuration.
#[tauri::command]
pub async fn test_remote_connection(
    config: AgentConfig,
    service: State<'_, Arc<CommandService>>,
) -> Result<(), CommandError> {
    service
        .execute(Command::TestRemoteConnection { config })
        .await?;
    Ok(())
}

/// Save configuration via AgentState, allowing tests to bypass Tauri app handle.
#[cfg(test)]
pub(crate) async fn save_config_for_state(
    config: AgentConfig,
    state: &AgentState,
) -> Result<AgentConfig, String> {
    state
        .save_config(config)
        .await
        .map_err(|error| error.to_string())
}

/// Enable or disable system autostart via Tauri autostart plugin.
fn apply_autostart(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let autolaunch = app.autolaunch();
    let is_enabled = autolaunch.is_enabled().map_err(|error| error.to_string())?;
    if enabled == is_enabled {
        return Ok(());
    }
    if enabled {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    }
    .map_err(|error| error.to_string())
}

fn check_service_port_available_for_host(host: &str, port: u16) -> Result<(), String> {
    if !(MIN_SERVICE_PORT..=MAX_SERVICE_PORT).contains(&port) {
        return Err(format!(
            "本地端口必须在 {MIN_SERVICE_PORT} 到 {MAX_SERVICE_PORT} 之间"
        ));
    }

    TcpListener::bind((host, port))
        .map(drop)
        .map_err(|_| format!("本地端口 {port} 已被占用，请换一个端口"))
}

/// Return recent task logs to Tauri frontend.
#[tauri::command]
pub async fn get_logs(
    service: State<'_, Arc<CommandService>>,
) -> Result<Vec<TaskLogEntry>, CommandError> {
    match service.execute(Command::GetLogs).await? {
        CommandResult::Logs(logs) => Ok(logs),
        _ => unreachable!("GetLogs returned an unexpected result"),
    }
}

/// Return local task history summary to Tauri frontend.
#[tauri::command]
pub async fn get_task_history(
    service: State<'_, Arc<CommandService>>,
) -> Result<Vec<TaskHistoryJob>, CommandError> {
    match service.execute(Command::GetTaskHistory).await? {
        CommandResult::TaskHistory(jobs) => Ok(jobs),
        _ => unreachable!("GetTaskHistory returned an unexpected result"),
    }
}

/// Return historical status events for specified task to Tauri frontend.
#[tauri::command]
pub async fn get_task_history_events(
    job_id: String,
    service: State<'_, Arc<CommandService>>,
) -> Result<Vec<TaskHistoryEvent>, CommandError> {
    match service
        .execute(Command::GetTaskHistoryEvents { job_id })
        .await?
    {
        CommandResult::TaskHistoryEvents(events) => Ok(events),
        _ => unreachable!("GetTaskHistoryEvents returned an unexpected result"),
    }
}

/// Clear local task logs and task history.
#[tauri::command]
pub async fn clear_task_history(
    service: State<'_, Arc<CommandService>>,
) -> Result<(), CommandError> {
    service.execute(Command::ClearTaskHistory).await?;
    Ok(())
}

/// Read task history summary via AgentState, allowing tests to bypass Tauri State.
#[cfg(test)]
pub(crate) fn get_task_history_for_state(
    state: &AgentState,
) -> Result<Vec<TaskHistoryJob>, String> {
    let Some(store) = &state.task_history else {
        return Ok(Vec::new());
    };
    store.recent_jobs(500).map_err(|error| error.to_string())
}

/// Read specified task events via AgentState, allowing tests to bypass Tauri State.
#[cfg(test)]
pub(crate) fn get_task_history_events_for_state(
    job_id: &str,
    state: &AgentState,
) -> Result<Vec<TaskHistoryEvent>, String> {
    let Some(store) = &state.task_history else {
        return Ok(Vec::new());
    };
    store
        .events_for_job(job_id)
        .map_err(|error| error.to_string())
}

/// Clear task history via AgentState, allowing tests to bypass Tauri State.
#[cfg(test)]
pub(crate) async fn clear_task_history_for_state(state: &AgentState) -> Result<(), String> {
    state.logs.lock().await.clear();

    let Some(store) = &state.task_history else {
        return Ok(());
    };
    store.clear().map_err(|error| error.to_string())
}

/// Submit a calibration test page using current Agent default print settings.
#[tauri::command]
pub async fn print_test(
    config: AgentConfig,
    service: State<'_, Arc<CommandService>>,
) -> Result<(), CommandError> {
    service.execute(Command::TestPrint { config }).await?;
    Ok(())
}

/// Return list of system printers.
#[tauri::command]
pub async fn list_printers(
    service: State<'_, Arc<CommandService>>,
) -> Result<Vec<PrinterInfo>, CommandError> {
    match service.execute(Command::ListPrinters).await? {
        CommandResult::Printers(printers) => Ok(printers),
        _ => unreachable!("ListPrinters returned an unexpected result"),
    }
}

/// Return papers supported by specified printer.
#[tauri::command]
pub async fn list_papers(
    printer_name: String,
    service: State<'_, Arc<CommandService>>,
) -> Result<Vec<PaperInfo>, CommandError> {
    match service
        .execute(Command::ListPapers { printer_name })
        .await?
    {
        CommandResult::Papers(papers) => Ok(papers),
        _ => unreachable!("ListPapers returned an unexpected result"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        check_service_port_available_for_host, clear_task_history_for_state,
        get_task_history_events_for_state, get_task_history_for_state, run_doctor_with_service,
        save_config_for_state, MAX_SERVICE_PORT, MIN_SERVICE_PORT,
    };
    use crate::{
        config::AgentConfig,
        logs::TaskLogEntry,
        protocol::JobStatus,
        state::AgentState,
        task_history::{
            NewTaskHistoryEvent, TaskHistorySource, TaskHistoryStatus, TaskHistoryStore,
        },
    };
    use async_trait::async_trait;
    use print_bridge_cli::{
        Command, CommandError, CommandExecutor, CommandResult, CommandService, DoctorCheck,
        DoctorReport, DoctorStatus, ProductKind,
    };
    use std::{
        fs,
        sync::{Arc, Mutex},
    };

    struct RecordingDoctorExecutor {
        calls: Arc<Mutex<Vec<Command>>>,
        report: DoctorReport,
    }

    #[async_trait]
    impl CommandExecutor for RecordingDoctorExecutor {
        async fn execute(&self, command: Command) -> Result<CommandResult, CommandError> {
            self.calls.lock().unwrap().push(command);
            Ok(CommandResult::Doctor(self.report.clone()))
        }
    }

    #[tokio::test]
    async fn desktop_doctor_uses_the_shared_desktop_command() {
        let report = DoctorReport::new(vec![DoctorCheck {
            code: "service.port".to_string(),
            status: DoctorStatus::Pass,
            message: "Service port is active.".to_string(),
            suggestion: None,
        }]);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let executor: Arc<dyn CommandExecutor> = Arc::new(RecordingDoctorExecutor {
            calls: calls.clone(),
            report: report.clone(),
        });
        let service = CommandService::new(Some(executor.clone()), executor);

        let actual = run_doctor_with_service(&service).await.unwrap();

        assert_eq!(actual, report);
        assert_eq!(
            *calls.lock().unwrap(),
            vec![Command::Doctor {
                product: ProductKind::Desktop,
            }]
        );
    }

    #[tokio::test]
    async fn save_config_persists_to_state_config_path() {
        let path = std::env::temp_dir().join(format!(
            "print-bridge-command-save-config-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let state = AgentState::with_config_path(AgentConfig::default(), path.clone());
        let mut config = AgentConfig::default();
        config.service.port = 19090;

        save_config_for_state(config.clone(), &state).await.unwrap();

        let loaded = AgentConfig::load(&path).unwrap();
        assert_eq!(loaded, config);

        let _ = fs::remove_file(&path);
    }

    #[tokio::test]
    async fn save_config_for_state_normalizes_service_host_before_persisting() {
        let path = std::env::temp_dir().join(format!(
            "print-bridge-command-save-config-normalized-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let state = AgentState::with_config_path(AgentConfig::default(), path.clone());
        let mut config = AgentConfig::default();
        config.service.host = "0.0.0.0".to_string();
        config.service.port = 19091;

        let saved = save_config_for_state(config, &state).await.unwrap();

        assert_eq!(saved.service.host, "127.0.0.1");
        assert_eq!(state.config.read().await.service.host, "127.0.0.1");
        let loaded = AgentConfig::load(&path).unwrap();
        assert_eq!(loaded.service.host, "127.0.0.1");
        assert_eq!(loaded.service.port, 19091);

        let _ = fs::remove_file(&path);
    }

    #[tokio::test]
    async fn save_config_for_state_rejects_invalid_allowed_origins() {
        let path = std::env::temp_dir().join(format!(
            "print-bridge-command-save-config-invalid-origin-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let state = AgentState::with_config_path(AgentConfig::default(), path.clone());
        let mut config = AgentConfig::default();
        config.security.allowed_origins = vec!["not-a-url".to_string()];

        let error = save_config_for_state(config, &state).await.unwrap_err();

        assert_eq!(error, "invalid origin");
        assert!(!path.exists());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn check_service_port_available_rejects_occupied_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let error = check_service_port_available_for_host("127.0.0.1", port).unwrap_err();

        assert_eq!(error, format!("本地端口 {port} 已被占用，请换一个端口"));
    }

    #[test]
    fn check_service_port_available_rejects_ports_below_minimum() {
        let error =
            check_service_port_available_for_host("127.0.0.1", MIN_SERVICE_PORT - 1).unwrap_err();

        assert_eq!(
            error,
            format!("本地端口必须在 {MIN_SERVICE_PORT} 到 {MAX_SERVICE_PORT} 之间")
        );
    }

    #[tokio::test]
    async fn task_history_commands_return_empty_without_store() {
        let state = AgentState::new(AgentConfig::default());

        assert!(get_task_history_for_state(&state).unwrap().is_empty());
        assert!(get_task_history_events_for_state("job-1", &state)
            .unwrap()
            .is_empty());
        clear_task_history_for_state(&state).await.unwrap();
    }

    #[tokio::test]
    async fn task_history_commands_read_events_and_clear_store() {
        let store = TaskHistoryStore::open_in_memory().unwrap();
        store
            .record_event(&NewTaskHistoryEvent {
                job_id: "job-1",
                request_id: Some("request-1"),
                batch_id: None,
                source: TaskHistorySource::Test,
                status: TaskHistoryStatus::Queued,
                message: Some("queued"),
                printer_name: Some("Office Printer"),
                paper_name: Some("A4"),
                copies: Some(1),
                occurred_at: "2026-07-06T10:00:00Z",
            })
            .unwrap();
        let state = AgentState::new(AgentConfig::default()).with_task_history_store(store);

        let jobs = get_task_history_for_state(&state).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].job_id, "job-1");
        assert_eq!(jobs[0].source, TaskHistorySource::Test);

        let events = get_task_history_events_for_state("job-1", &state).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].status, TaskHistoryStatus::Queued);

        clear_task_history_for_state(&state).await.unwrap();
        assert!(get_task_history_for_state(&state).unwrap().is_empty());
        assert!(get_task_history_events_for_state("job-1", &state)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn clear_task_history_for_state_clears_recent_logs() {
        let state = AgentState::new(AgentConfig::default());
        state.logs.lock().await.push(TaskLogEntry {
            timestamp: "2026-07-06T10:00:00Z".to_string(),
            request_id: Some("request-1".to_string()),
            batch_id: None,
            job_id: Some("job-1".to_string()),
            origin: Some("http://localhost:5173".to_string()),
            status: JobStatus::Queued,
            message: "queued".to_string(),
        });

        clear_task_history_for_state(&state).await.unwrap();

        assert!(state.logs.lock().await.recent().is_empty());
    }
}
