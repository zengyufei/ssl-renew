use serde_json::Value;
use ssl_core::config::{
    environment_group_status, load_store, normalize_store, profiles_path,
    resolve_profile_environment_group, save_store, EnvironmentGroupStatus, Store,
};
use ssl_core::monitor::{next_monitor_run, selected_profiles};
use ssl_core::signer::{
    authorize_via_pipe, default_secrets_path, init_config, lock_via_pipe, signer_status,
    status_via_pipe, unlock_via_pipe, SignerInitRequest,
};
use ssl_core::workflow;
use ssl_core::DnsProviderKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::watch;

#[derive(Clone)]
struct MonitorControl {
    stop: Arc<Mutex<Option<watch::Sender<bool>>>>,
}

#[tauri::command]
fn load_profiles() -> Result<Store, String> {
    load_store(profiles_path()).map_err(|err| err.to_string())
}

#[tauri::command]
fn save_profiles(store: Store) -> Result<(), String> {
    save_store(profiles_path(), &store).map_err(|err| err.to_string())
}

#[tauri::command]
fn export_profiles_yaml() -> Result<String, String> {
    std::fs::read_to_string(profiles_path())
        .map_err(|err| format!("导出 profiles.yaml 失败：{err}"))
}

#[tauri::command]
fn export_profiles_yaml_to_file() -> Result<Option<String>, String> {
    let source = profiles_path();
    let mut dialog = rfd::FileDialog::new()
        .set_title("导出 profiles.yaml")
        .set_file_name("profiles.yaml")
        .add_filter("YAML 文件", &["yaml", "yml"]);
    if let Ok(current_dir) = std::env::current_dir() {
        dialog = dialog.set_directory(current_dir);
    }
    let Some(target) = dialog.save_file() else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(&source)
        .map_err(|err| format!("读取 profiles.yaml 失败：{err}"))?;
    std::fs::write(&target, text).map_err(|err| format!("写入导出文件失败：{err}"))?;
    Ok(Some(target.display().to_string()))
}

#[tauri::command]
fn import_profiles_yaml(text: String) -> Result<Store, String> {
    let mut store: Store =
        serde_yaml::from_str(&text).map_err(|err| format!("导入 YAML 解析失败：{err}"))?;
    normalize_store(&mut store);
    save_store(profiles_path(), &store).map_err(|err| err.to_string())?;
    Ok(store)
}

#[tauri::command]
fn init_signer_cmd(request: SignerInitRequest) -> Result<String, String> {
    let path = default_secrets_path();
    init_config(&path, request).map_err(|err| err.to_string())?;
    Ok(format!("signer 初始化完成：{}", path.display()))
}

#[tauri::command]
fn signer_status_cmd() -> Result<String, String> {
    signer_status(default_secrets_path()).map_err(|err| err.to_string())
}

#[tauri::command]
async fn signer_runtime_status_cmd(pipe_name: String) -> Result<String, String> {
    let response = status_via_pipe(&pipe_name)
        .await
        .map_err(|err| err.to_string())?;
    if response.ok {
        Ok(response.message)
    } else {
        Err(response.message)
    }
}

#[tauri::command]
async fn unlock_signer_cmd(pipe_name: String, passphrase: String) -> Result<String, String> {
    let response = unlock_via_pipe(&pipe_name, passphrase)
        .await
        .map_err(|err| err.to_string())?;
    if response.ok {
        Ok(response.message)
    } else {
        Err(response.message)
    }
}

#[tauri::command]
async fn lock_signer_cmd(pipe_name: String) -> Result<String, String> {
    let response = lock_via_pipe(&pipe_name)
        .await
        .map_err(|err| err.to_string())?;
    if response.ok {
        Ok(response.message)
    } else {
        Err(response.message)
    }
}

#[tauri::command]
async fn signer_authorize_test_cmd(pipe_name: String) -> Result<String, String> {
    let response = authorize_via_pipe(&pipe_name)
        .await
        .map_err(|err| err.to_string())?;
    if response.ok {
        Ok(response.message)
    } else {
        Err(response.message)
    }
}

#[tauri::command]
async fn check_certificate_cmd(domain: String, force: bool) -> Result<Value, String> {
    let profile = profile(&domain)?;
    let status = workflow::check_certificate(&profile, force)
        .await
        .map_err(|err| err.to_string())?;
    serde_json::to_value(status).map_err(|err| err.to_string())
}

#[tauri::command]
async fn create_order_cmd(domain: String) -> Result<Value, String> {
    let profile = runtime_profile(&domain)?;
    let runtime = workflow::create_order_prepare_dns(&profile)
        .await
        .map_err(|err| err.to_string())?;
    serde_json::to_value(runtime.session.challenges).map_err(|err| err.to_string())
}

#[tauri::command]
fn environment_group_status_cmd(domain: String) -> Result<Option<EnvironmentGroupStatus>, String> {
    let store = load_store(profiles_path()).map_err(|err| err.to_string())?;
    let profile = store
        .profiles
        .get(&domain)
        .ok_or_else(|| format!("找不到配置：{domain}"))?;
    environment_group_status(&store, profile).map_err(|err| err.to_string())
}

#[tauri::command]
async fn dns_check_cmd(domain: String) -> Result<bool, String> {
    let profile = profile(&domain)?;
    let challenges = workflow::load_saved_challenges(&profile).map_err(|err| err.to_string())?;
    workflow::dns_records_visible(&profile, &challenges)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn issue_cmd(domain: String) -> Result<(), String> {
    let profile = profile(&domain)?;
    workflow::issue_certificate(&profile)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn restart_cmd(domain: String) -> Result<(), String> {
    let profile = profile(&domain)?;
    workflow::restart_nginx_for_profile(&profile)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn start_monitor_cmd(
    app: AppHandle,
    control: State<'_, MonitorControl>,
) -> Result<(), String> {
    stop_monitor_cmd(control.clone())?;
    let (tx, rx) = watch::channel(false);
    *control.stop.lock().map_err(|_| "监控状态锁失败")? = Some(tx);
    tauri::async_runtime::spawn(async move {
        run_monitor(app, rx).await;
    });
    Ok(())
}

#[tauri::command]
fn stop_monitor_cmd(control: State<'_, MonitorControl>) -> Result<(), String> {
    if let Some(tx) = control.stop.lock().map_err(|_| "监控状态锁失败")?.take() {
        let _ = tx.send(true);
    }
    Ok(())
}

#[tauri::command]
fn open_github_profile() -> Result<(), String> {
    let url = "https://github.com/zengyufei/ssl-renew";
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    command
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("打开 GitHub 失败：{err}"))
}

#[tauri::command]
fn open_path_folder(path: String) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("请先填写文件路径".to_string());
    }
    let requested = PathBuf::from(path);
    let folder = if requested.is_dir() {
        requested
    } else {
        requested
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    };
    if !folder.is_dir() {
        return Err(format!("文件夹不存在：{}", folder.display()));
    }

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("explorer.exe");
        command.arg(&folder);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(&folder);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(&folder);
        command
    };
    command
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("打开文件夹失败：{err}"))
}

async fn run_monitor(app: AppHandle, mut stop: watch::Receiver<bool>) {
    loop {
        let store = match load_store(profiles_path()) {
            Ok(store) => store,
            Err(err) => {
                emit(&app, format!("读取监控配置失败：{err}"));
                return;
            }
        };
        let next = match next_monitor_run(&store.monitor) {
            Ok(next) => next,
            Err(err) => {
                emit(&app, format!("监控配置无效：{err}"));
                return;
            }
        };
        emit(
            &app,
            format!(
                "监控已启动，下一次执行时间：{}",
                next.format("%Y-%m-%d %H:%M:%S")
            ),
        );
        let wait_ms = (next - chrono::Local::now()).num_milliseconds().max(1000) as u64;
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(wait_ms)) => {}
            _ = stop.changed() => {
                emit(&app, "监控已停止".to_string());
                return;
            }
        }
        let store = match load_store(profiles_path()) {
            Ok(store) => store,
            Err(err) => {
                emit(&app, format!("读取监控配置失败：{err}"));
                continue;
            }
        };
        for (domain, profile) in selected_profiles(&store.monitor, &store.profiles) {
            emit(&app, format!("开始监控配置：{domain}"));
            if DnsProviderKind::from_value(&profile.dns.provider) == DnsProviderKind::Manual {
                emit(&app, format!("手动DNS 无法无人值守续期，已跳过：{domain}"));
                continue;
            }
            let runtime_profile = match resolve_profile_environment_group(&store, profile) {
                Ok(profile) => profile,
                Err(err) => {
                    emit(&app, format!("{}：执行失败：{err:#}", domain));
                    continue;
                }
            };
            match workflow::renew_profile(&runtime_profile, false).await {
                Ok(outcome) => emit(&app, format!("{}：{}", domain, outcome.message)),
                Err(err) => emit(&app, format!("{}：执行失败：{err:#}", domain)),
            }
        }
    }
}

fn profile(domain: &str) -> Result<ssl_core::config::Profile, String> {
    let store = load_store(profiles_path()).map_err(|err| err.to_string())?;
    store
        .profiles
        .get(domain)
        .cloned()
        .ok_or_else(|| format!("找不到配置：{domain}"))
}

fn runtime_profile(domain: &str) -> Result<ssl_core::config::Profile, String> {
    let store = load_store(profiles_path()).map_err(|err| err.to_string())?;
    let profile = store
        .profiles
        .get(domain)
        .ok_or_else(|| format!("找不到配置：{domain}"))?;
    resolve_profile_environment_group(&store, profile).map_err(|err| err.to_string())
}

fn emit(app: &AppHandle, text: String) {
    let _ = app.emit("backend-log", text);
}

pub fn run() {
    tauri::Builder::default()
        .manage(MonitorControl {
            stop: Arc::new(Mutex::new(None)),
        })
        .invoke_handler(tauri::generate_handler![
            load_profiles,
            save_profiles,
            export_profiles_yaml,
            export_profiles_yaml_to_file,
            import_profiles_yaml,
            init_signer_cmd,
            signer_status_cmd,
            signer_runtime_status_cmd,
            unlock_signer_cmd,
            lock_signer_cmd,
            signer_authorize_test_cmd,
            check_certificate_cmd,
            create_order_cmd,
            environment_group_status_cmd,
            dns_check_cmd,
            issue_cmd,
            restart_cmd,
            start_monitor_cmd,
            stop_monitor_cmd,
            open_github_profile,
            open_path_folder
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用失败");
}
