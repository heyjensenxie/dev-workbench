mod api;
mod database;
mod error;
mod manifest;
mod models;
mod repository;
mod scanner;
mod vault;

use error::AppError;
use models::{
    ApiModule, DevService, Project, ProjectContext, RunningService, SavedApiRequest, ServiceState,
    now_millis,
};
use sqlx::SqlitePool;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};
use tauri::{Emitter, Manager, State};
use workbench_process::{ProcessRuntime, ProcessSpec};

/// How often a running service is checked for an unexpected exit.
const EXIT_WATCH_INTERVAL: Duration = Duration::from_millis(750);

struct AppState {
    database: SqlitePool,
    processes: Arc<ProcessRuntime>,
}

#[tauri::command]
async fn list_projects(state: State<'_, AppState>) -> Result<Vec<Project>, AppError> {
    repository::list_projects(&state.database).await
}

#[tauri::command]
async fn open_project(path: String, state: State<'_, AppState>) -> Result<Project, AppError> {
    let root = Path::new(&path);
    if !root.is_dir() {
        return Err(AppError::Validation(
            "project path must be an existing directory".into(),
        ));
    }
    let canonical = root.canonicalize()?.to_string_lossy().into_owned();
    let now = now_millis();
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Project")
        .to_owned();
    let existing = repository::list_projects(&state.database)
        .await?
        .into_iter()
        .find(|project| project.path == canonical);
    let project = Project {
        id: existing
            .as_ref()
            .map(|project| project.id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        name,
        path: canonical,
        created_at: existing.as_ref().map_or(now, |project| project.created_at),
        updated_at: now,
        last_opened_at: Some(now),
    };
    repository::upsert_project(&state.database, &project).await?;
    Ok(project)
}

/// Removes a project from the workbench and stops services owned by it.
/// This intentionally never deletes anything below the project's path.
#[tauri::command]
async fn delete_project(project_id: String, state: State<'_, AppState>) -> Result<bool, AppError> {
    let services = repository::list_services(&state.database, &project_id).await?;
    for service in services {
        if state.processes.is_running(&service.id).await {
            state.processes.stop(&service.id).await?;
        }
    }
    repository::delete_project(&state.database, &project_id).await
}

/// Opens the project directory in the host file manager.
#[tauri::command]
async fn reveal_project(path: String) -> Result<(), AppError> {
    let root = Path::new(&path);
    if !root.is_dir() {
        return Err(AppError::Validation(
            "project path must be an existing directory".into(),
        ));
    }
    #[cfg(target_os = "windows")]
    let mut command = Command::new("explorer");
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = Command::new("xdg-open");
    command.arg(root);
    command.spawn()?;
    Ok(())
}

#[tauri::command]
async fn scan_project(project: Project) -> Result<ProjectContext, AppError> {
    let root = Path::new(&project.path);
    if !root.is_dir() {
        return Err(AppError::Validation(
            "project path must be an existing directory".into(),
        ));
    }
    Ok(scanner::scan(&project))
}

#[tauri::command]
async fn list_services(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<DevService>, AppError> {
    repository::list_services(&state.database, &project_id).await
}

#[tauri::command]
async fn save_service(service: DevService, state: State<'_, AppState>) -> Result<(), AppError> {
    repository::save_service(&state.database, &service).await
}

/// Deletes a service and detaches it from the dependency list of its siblings.
#[tauri::command]
async fn delete_service(service_id: String, state: State<'_, AppState>) -> Result<bool, AppError> {
    let existing = repository::get_service(&state.database, &service_id).await?;
    let Some(service) = existing else {
        return Ok(false);
    };
    if state.processes.is_running(&service_id).await {
        state.processes.stop(&service_id).await?;
    }
    let deleted = repository::delete_service(&state.database, &service_id).await?;
    if deleted {
        repository::forget_service_dependencies(&state.database, &service.project_id, &service_id)
            .await?;
    }
    Ok(deleted)
}

#[tauri::command]
async fn start_service(
    service: DevService,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<ServiceState, AppError> {
    service.validate().map_err(AppError::Validation)?;
    // Give the user a useful domain error before spawning when the configured
    // port is already occupied. Port inspection is best-effort because lsof
    // may not be installed on a minimal Unix workstation.
    if let Some(port) = service.port
        && let Ok(ports) = workbench_network::list_ports()
        && let Some(occupied) = ports.into_iter().find(|item| item.port == port)
    {
        let owner = occupied
            .process_name
            .or_else(|| occupied.pid.map(|pid| format!("pid {pid}")))
            .unwrap_or_else(|| "another process".into());
        return Err(AppError::Validation(format!(
            "port {port} is already in use by {owner}"
        )));
    }
    let project = repository::get_project(&state.database, &service.project_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("project {}", service.project_id)))?;
    let spec = ProcessSpec {
        id: service.id.clone(),
        command: service.command.trim().to_owned(),
        args: service.args.clone(),
        cwd: Some(
            resolve_service_cwd(&project.path, service.cwd.as_deref())
                .to_string_lossy()
                .into_owned(),
        ),
        env: service.env.clone(),
    };
    let service_id = service.id.clone();
    let emitter = app.clone();
    let pid = state
        .processes
        .start(spec, move |event| {
            let _ = emitter.emit("service-log", event);
        })
        .await?;
    watch_service_exit(state.processes.clone(), app, service_id.clone());
    Ok(ServiceState {
        service_id,
        status: "running".into(),
        pid: Some(pid),
    })
}

/// Resolves a service directory at the native boundary so imported scanner
/// suggestions can remain portable project-relative paths.
fn resolve_service_cwd(project_path: &str, cwd: Option<&str>) -> PathBuf {
    let Some(cwd) = cwd.map(str::trim).filter(|cwd| !cwd.is_empty()) else {
        return PathBuf::from(project_path);
    };
    let path = Path::new(cwd);
    if path.is_absolute() {
        path.to_owned()
    } else {
        Path::new(project_path).join(path)
    }
}

#[tauri::command]
async fn stop_service(
    service_id: String,
    state: State<'_, AppState>,
) -> Result<ServiceState, AppError> {
    state.processes.stop(&service_id).await?;
    Ok(ServiceState {
        service_id,
        status: "stopped".into(),
        pid: None,
    })
}

#[tauri::command]
async fn stop_all_services(state: State<'_, AppState>) -> Result<usize, AppError> {
    Ok(state.processes.stop_all().await)
}

/// Reports the services the runtime still owns, so the UI can restore state
/// after a reload or a window crash.
#[tauri::command]
async fn list_running_services(
    state: State<'_, AppState>,
) -> Result<Vec<RunningService>, AppError> {
    Ok(state
        .processes
        .running()
        .await
        .into_iter()
        .map(|process| RunningService {
            service_id: process.service_id,
            pid: process.pid,
        })
        .collect())
}

#[tauri::command]
async fn list_processes() -> Result<Vec<workbench_system::ProcessInfo>, AppError> {
    Ok(workbench_system::list_processes())
}

#[tauri::command]
async fn process_tree(pid: u32) -> Result<Vec<workbench_system::ProcessInfo>, AppError> {
    let tree = workbench_system::process_tree(pid);
    if tree.is_empty() {
        return Err(AppError::NotFound(format!("process {pid}")));
    }
    Ok(tree)
}

/// Terminates a process together with every descendant it spawned.
#[tauri::command]
async fn kill_process(pid: u32) -> Result<(), AppError> {
    workbench_system::kill_process_tree(pid)
        .then_some(())
        .ok_or_else(|| AppError::NotFound(format!("process {pid}")))
}

#[tauri::command]
async fn list_ports() -> Result<Vec<workbench_network::PortInfo>, AppError> {
    Ok(workbench_network::list_ports()?)
}

#[tauri::command]
async fn kill_port(port: u16) -> Result<(), AppError> {
    let item = workbench_network::list_ports()?
        .into_iter()
        .find(|item| item.port == port)
        .ok_or_else(|| AppError::NotFound(format!("port {port}")))?;
    let pid = item
        .pid
        .ok_or_else(|| AppError::NotFound(format!("process for port {port}")))?;
    kill_process(pid).await
}

/// Returns every persisted preference, defaulted by the caller.
#[tauri::command]
async fn get_settings(
    state: State<'_, AppState>,
) -> Result<BTreeMap<String, serde_json::Value>, AppError> {
    repository::load_settings(&state.database).await
}

#[tauri::command]
async fn set_setting(
    key: String,
    value: serde_json::Value,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    repository::save_setting(&state.database, &key, &value).await
}

#[tauri::command]
async fn test_database_connection(
    config: database::DatabaseConnectionConfig,
) -> Result<database::ConnectionTestResult, AppError> {
    database::test_connection(config).await
}

#[tauri::command]
async fn list_database_databases(
    config: database::DatabaseConnectionConfig,
) -> Result<Vec<database::DatabaseInfo>, AppError> {
    database::list_databases(config).await
}

#[tauri::command]
async fn list_database_tables(
    config: database::DatabaseConnectionConfig,
    database: String,
) -> Result<Vec<database::TableInfo>, AppError> {
    database::list_tables(config, database).await
}

#[tauri::command]
async fn query_database(
    config: database::DatabaseConnectionConfig,
    sql: String,
) -> Result<database::QueryResult, AppError> {
    database::query(config, sql).await
}

#[tauri::command]
fn store_database_password(connection_id: String, password: String) -> Result<(), AppError> {
    workbench_secrets::write(
        &format!("DevWorkbench/Database/{connection_id}"),
        "password",
        &password,
    )
    .map_err(AppError::Validation)
}

#[tauri::command]
fn read_database_password(connection_id: String) -> Result<Option<String>, AppError> {
    workbench_secrets::read(&format!("DevWorkbench/Database/{connection_id}"))
        .map_err(AppError::Validation)
}

#[tauri::command]
fn delete_database_password(connection_id: String) -> Result<(), AppError> {
    workbench_secrets::delete(&format!("DevWorkbench/Database/{connection_id}"))
        .map_err(AppError::Validation)
}

#[tauri::command]
async fn http_request(request: api::HttpRequest) -> Result<api::HttpResponse, AppError> {
    Ok(api::send(request).await?)
}

#[tauri::command]
async fn list_api_modules(state: State<'_, AppState>) -> Result<Vec<ApiModule>, AppError> {
    repository::list_api_modules(&state.database).await
}

#[tauri::command]
async fn save_api_module(
    module: ApiModule,
    state: State<'_, AppState>,
) -> Result<ApiModule, AppError> {
    repository::save_api_module(&state.database, &module).await
}

#[tauri::command]
async fn delete_api_module(
    module_id: String,
    state: State<'_, AppState>,
) -> Result<bool, AppError> {
    repository::delete_api_module(&state.database, &module_id).await
}

#[tauri::command]
async fn list_api_requests(
    module_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<SavedApiRequest>, AppError> {
    repository::list_api_requests(&state.database, module_id.as_deref()).await
}

#[tauri::command]
async fn save_api_request(
    request: SavedApiRequest,
    state: State<'_, AppState>,
) -> Result<SavedApiRequest, AppError> {
    repository::save_api_request(&state.database, &request).await
}

#[tauri::command]
async fn delete_api_request(
    request_id: String,
    state: State<'_, AppState>,
) -> Result<bool, AppError> {
    repository::delete_api_request(&state.database, &request_id).await
}

/// Emits `service-exit` once a started service stops, whichever way it ended.
fn watch_service_exit(processes: Arc<ProcessRuntime>, app: tauri::AppHandle, service_id: String) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(EXIT_WATCH_INTERVAL).await;
            if !processes.is_running(&service_id).await {
                let _ = app.emit("service-exit", service_id);
                break;
            }
        }
    });
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data = app.path().app_data_dir()?;
            let db_path = app_data.join("workbench.sqlite3");
            let database = tauri::async_runtime::block_on(repository::connect(&db_path))
                .map_err(|error| Box::<dyn std::error::Error>::from(error.to_string()))?;

            // The vault lives in its own database file, separate from the
            // workspace database, and is managed by its own service. It shares no
            // table, no connection, and no trust boundary with anything above.
            let vault_path = app_data.join("vault.db");
            let vault_service = tauri::async_runtime::block_on(workbench_vault::VaultService::open(
                &vault_path,
            ))
            .map_err(|error| Box::<dyn std::error::Error>::from(error.to_string()))?;

            app.manage(AppState {
                database,
                processes: ProcessRuntime::new(),
            });
            // The safety copy taken before a restore-replace is written beside the
            // vault, so it travels with the application data rather than with
            // whatever directory the process was started from.
            app.manage(vault::VaultState::new(vault_service, app_data.clone()));
            vault::spawn_auto_lock_supervisor(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_projects,
            open_project,
            delete_project,
            reveal_project,
            scan_project,
            list_services,
            save_service,
            delete_service,
            start_service,
            stop_service,
            stop_all_services,
            list_running_services,
            list_processes,
            process_tree,
            kill_process,
            list_ports,
            kill_port,
            get_settings,
            set_setting,
            test_database_connection,
            list_database_databases,
            list_database_tables,
            query_database,
            store_database_password,
            read_database_password,
            delete_database_password,
            http_request,
            list_api_modules,
            save_api_module,
            delete_api_module,
            list_api_requests,
            save_api_request,
            delete_api_request,
            vault::vault_status,
            vault::vault_create,
            vault::vault_unlock,
            vault::vault_lock,
            vault::vault_touch,
            vault::vault_set_auto_lock,
            vault::vault_list_items,
            vault::vault_get_item,
            vault::vault_create_item,
            vault::vault_update_item,
            vault::vault_delete_item,
            vault::vault_destroy,
            vault::vault_set_favorite,
            vault::vault_reveal_field,
            vault::vault_copy_item_field,
            vault::vault_change_master_password,
            vault::vault_recalibrate,
            vault::vault_export_backup,
            vault::vault_import_backup,
            vault::vault_generate_password,
            vault::vault_copy_secret,
            vault::vault_clear_clipboard,
            vault::vault_open_url
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Dev Workbench");
    app.run(|handle, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            let processes = handle.state::<AppState>().processes.clone();
            tauri::async_runtime::block_on(processes.stop_all());
            // Wipe the vault master key and any password left on the clipboard
            // rather than relying on process teardown to do it.
            if let Some(vault) = handle.try_state::<vault::VaultState>() {
                vault::lock_on_exit(&vault);
            }
            let _ = workbench_vault::clipboard::clear();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_missing_working_directory_to_project_root() {
        assert_eq!(
            resolve_service_cwd("/workspace/demo", None),
            PathBuf::from("/workspace/demo")
        );
    }

    #[test]
    fn resolves_relative_working_directory_against_project_root() {
        let resolved = resolve_service_cwd("/workspace/demo", Some("apps/web"));
        assert_eq!(resolved, PathBuf::from("/workspace/demo").join("apps/web"));
    }

    #[test]
    fn preserves_absolute_working_directory() {
        let resolved = resolve_service_cwd("/workspace/demo", Some("/tmp/service"));
        assert_eq!(resolved, PathBuf::from("/tmp/service"));
    }
}
