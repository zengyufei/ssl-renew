use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const PROFILES_PATH: &str = "profiles.yaml";

pub fn profiles_path() -> PathBuf {
    if let Ok(current) = std::env::current_dir() {
        for dir in current.ancestors() {
            let candidate = dir.join(PROFILES_PATH);
            if candidate.exists() {
                return candidate;
            }
        }
        return current.join(PROFILES_PATH);
    }
    PathBuf::from(PROFILES_PATH)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Store {
    pub current_domain: String,
    #[serde(default = "default_vendor_configs")]
    pub vendor_configs: BTreeMap<String, Vec<VendorEnvEntry>>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
    #[serde(default = "default_monitor_config")]
    pub monitor: MonitorConfig,
    #[serde(default = "default_app_settings")]
    pub app_settings: AppSettings,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VendorEnvEntry {
    pub alias: String,
    pub key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub domain: String,
    pub email: String,
    #[serde(default = "default_renew")]
    pub renew: RenewConfig,
    #[serde(default)]
    pub paths: PathsConfig,
    #[serde(default)]
    pub dns: DnsConfig,
    #[serde(default)]
    pub nginx: NginxConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenewConfig {
    #[serde(default = "default_days_before_expiry")]
    pub days_before_expiry: i64,
    #[serde(default)]
    pub force: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PathsConfig {
    #[serde(default = "default_state_dir")]
    pub state_dir: String,
    #[serde(default = "default_work_dir")]
    pub work_dir: String,
    #[serde(default = "default_log_file")]
    pub log_file: String,
    #[serde(default)]
    pub cert_file: String,
    #[serde(default)]
    pub key_file: String,
    #[serde(default = "default_backup_dir")]
    pub backup_dir: String,
    #[serde(default = "default_max_log_size_mb")]
    pub max_log_size_mb: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DnsConfig {
    #[serde(default = "default_dns_provider")]
    pub provider: String,
    #[serde(default = "default_propagation_timeout")]
    pub propagation_timeout_seconds: u64,
    #[serde(default = "default_propagation_interval")]
    pub propagation_interval_seconds: u64,
    #[serde(default = "default_resolvers")]
    pub resolvers: Vec<String>,
    #[serde(default)]
    pub aliyun: AliyunConfig,
    #[serde(default)]
    pub cloudflare: CloudflareConfig,
    #[serde(default)]
    pub signer: SignerConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AliyunConfig {
    #[serde(default = "default_aliyun_key_env")]
    pub access_key_id_env: String,
    #[serde(default = "default_aliyun_secret_env")]
    pub access_key_secret_env: String,
    pub root_domain: Option<String>,
    #[serde(default = "default_aliyun_endpoint")]
    pub endpoint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CloudflareConfig {
    #[serde(default = "default_cf_token_env")]
    pub api_token_env: String,
    pub root_domain: Option<String>,
    #[serde(default = "default_cf_endpoint")]
    pub endpoint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignerConfig {
    #[serde(default = "default_signer_pipe")]
    pub pipe_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NginxConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_nginx_restart_mode")]
    pub restart_mode: String,
    #[serde(default = "default_nginx_exe")]
    pub exe_path: String,
    #[serde(default = "default_nginx_dir")]
    pub working_dir: String,
    #[serde(default = "default_nginx_image")]
    pub kill_image_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MonitorConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default = "default_monitor_mode")]
    pub mode: String,
    #[serde(default = "default_daily_time")]
    pub daily_time: String,
    #[serde(default = "default_interval_minutes")]
    pub interval_minutes: u64,
    #[serde(default = "default_cron_expression")]
    pub cron_expression: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_toast_settings")]
    pub toast: ToastSettings,
    #[serde(default = "default_notification_settings")]
    pub notification: NotificationSettings,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToastSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_toast_position")]
    pub position: String,
    #[serde(default = "default_toast_duration_ms")]
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NotificationSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_notification_channel")]
    pub channel: String,
    #[serde(default)]
    pub scope: NotificationScope,
    #[serde(default)]
    pub dingtalk: DingtalkNotification,
    #[serde(default)]
    pub telegram: TelegramNotification,
    #[serde(default)]
    pub feishu: FeishuNotification,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NotificationScope {
    #[serde(default)]
    pub step_check_success: bool,
    #[serde(default)]
    pub step_check_failure: bool,
    #[serde(default)]
    pub step_order_success: bool,
    #[serde(default)]
    pub step_order_failure: bool,
    #[serde(default)]
    pub step_dns_check_success: bool,
    #[serde(default)]
    pub step_dns_check_failure: bool,
    #[serde(default)]
    pub step_issue_success: bool,
    #[serde(default)]
    pub step_issue_failure: bool,
    #[serde(default)]
    pub step_restart_success: bool,
    #[serde(default)]
    pub step_restart_failure: bool,
    #[serde(default)]
    pub monitor_start: bool,
    #[serde(default)]
    pub monitor_stop: bool,
    #[serde(default)]
    pub monitor_profile_start: bool,
    #[serde(default)]
    pub monitor_no_renew_needed: bool,
    #[serde(default)]
    pub monitor_renew_needed: bool,
    #[serde(default)]
    pub monitor_manual_dns_skipped: bool,
    #[serde(default)]
    pub monitor_full_success: bool,
    #[serde(default)]
    pub monitor_full_failure: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DingtalkNotification {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub secret: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TelegramNotification {
    #[serde(default)]
    pub bot_token: String,
    #[serde(default)]
    pub chat_id: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FeishuNotification {
    #[serde(default)]
    pub webhook_url: String,
    #[serde(default)]
    pub secret: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DnsProviderKind {
    Manual,
    Aliyun,
    Cloudflare,
    Signer,
}

impl DnsProviderKind {
    pub fn from_value(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "aliyun" | "ali" | "阿里云" => Self::Aliyun,
            "cloudflare" | "cf" => Self::Cloudflare,
            "signer" | "agent" | "签发程序" => Self::Signer,
            _ => Self::Manual,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Aliyun => "aliyun",
            Self::Cloudflare => "cloudflare",
            Self::Signer => "signer",
        }
    }
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            state_dir: default_state_dir(),
            work_dir: default_work_dir(),
            log_file: default_log_file(),
            cert_file: String::new(),
            key_file: String::new(),
            backup_dir: default_backup_dir(),
            max_log_size_mb: default_max_log_size_mb(),
        }
    }
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            provider: default_dns_provider(),
            propagation_timeout_seconds: default_propagation_timeout(),
            propagation_interval_seconds: default_propagation_interval(),
            resolvers: default_resolvers(),
            aliyun: AliyunConfig::default(),
            cloudflare: CloudflareConfig::default(),
            signer: SignerConfig::default(),
        }
    }
}

impl Default for AliyunConfig {
    fn default() -> Self {
        Self {
            access_key_id_env: default_aliyun_key_env(),
            access_key_secret_env: default_aliyun_secret_env(),
            root_domain: None,
            endpoint: default_aliyun_endpoint(),
        }
    }
}

impl Default for CloudflareConfig {
    fn default() -> Self {
        Self {
            api_token_env: default_cf_token_env(),
            root_domain: None,
            endpoint: default_cf_endpoint(),
        }
    }
}

impl Default for SignerConfig {
    fn default() -> Self {
        Self {
            pipe_name: default_signer_pipe(),
        }
    }
}

impl Default for NginxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            restart_mode: default_nginx_restart_mode(),
            exe_path: default_nginx_exe(),
            working_dir: default_nginx_dir(),
            kill_image_name: default_nginx_image(),
        }
    }
}

impl Default for MonitorConfig {
    fn default() -> Self {
        default_monitor_config()
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        default_app_settings()
    }
}

impl Default for ToastSettings {
    fn default() -> Self {
        default_toast_settings()
    }
}

impl Default for NotificationSettings {
    fn default() -> Self {
        default_notification_settings()
    }
}

pub fn load_store(path: impl AsRef<Path>) -> Result<Store> {
    let path = path.as_ref();
    if !path.exists() {
        let profile = default_profile("*.example.com");
        let current = profile.domain.clone();
        let mut profiles = BTreeMap::new();
        profiles.insert(current.clone(), profile);
        let store = Store {
            current_domain: current,
            vendor_configs: default_vendor_configs(),
            profiles,
            monitor: default_monitor_config(),
            app_settings: default_app_settings(),
        };
        save_store(path, &store)?;
        return Ok(store);
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("读取配置失败：{}", path.display()))?;
    let mut store: Store =
        serde_yaml::from_str(&text).with_context(|| "解析 profiles.yaml 失败")?;
    normalize_store(&mut store);
    Ok(store)
}

pub fn save_store(path: impl AsRef<Path>, store: &Store) -> Result<()> {
    let path = path.as_ref();
    let text = serde_yaml::to_string(store)?;
    fs::write(path, text).with_context(|| format!("保存配置失败：{}", path.display()))
}

pub fn normalize_store(store: &mut Store) {
    if store.profiles.is_empty() {
        let profile = default_profile("*.example.com");
        store.current_domain = profile.domain.clone();
        store.profiles.insert(profile.domain.clone(), profile);
    }
    if !store.profiles.contains_key(&store.current_domain) {
        store.current_domain = store.profiles.keys().next().cloned().unwrap_or_default();
    }
    for profile in store.profiles.values_mut() {
        if profile.paths.cert_file.is_empty() || profile.paths.key_file.is_empty() {
            let safe = safe_domain_filename(&profile.domain);
            if profile.paths.cert_file.is_empty() {
                profile.paths.cert_file = format!("D:/cert/{safe}.pem");
            }
            if profile.paths.key_file.is_empty() {
                profile.paths.key_file = format!("D:/cert/{safe}.key");
            }
        }
    }
    for (key, value) in default_vendor_configs() {
        store.vendor_configs.entry(key).or_insert(value);
    }
}

pub fn default_profile(domain: &str) -> Profile {
    let safe = safe_domain_filename(domain);
    let root = domain.strip_prefix("*.").unwrap_or(domain);
    Profile {
        domain: domain.to_string(),
        email: format!("admin@{root}"),
        renew: default_renew(),
        paths: PathsConfig {
            cert_file: format!("D:/cert/{safe}.pem"),
            key_file: format!("D:/cert/{safe}.key"),
            ..Default::default()
        },
        dns: Default::default(),
        nginx: Default::default(),
    }
}

pub fn default_vendor_configs() -> BTreeMap<String, Vec<VendorEnvEntry>> {
    BTreeMap::from([
        ("manual".to_string(), vec![]),
        (
            "aliyun".to_string(),
            vec![
                VendorEnvEntry {
                    alias: "AccessKeyId".to_string(),
                    key: "Ali_Key".to_string(),
                },
                VendorEnvEntry {
                    alias: "AccessKeySecret".to_string(),
                    key: "Ali_Secret".to_string(),
                },
            ],
        ),
        (
            "cloudflare".to_string(),
            vec![VendorEnvEntry {
                alias: "API Token".to_string(),
                key: "CF_Token".to_string(),
            }],
        ),
    ])
}

pub fn default_monitor_config() -> MonitorConfig {
    MonitorConfig {
        enabled: false,
        profiles: vec![],
        mode: default_monitor_mode(),
        daily_time: default_daily_time(),
        interval_minutes: default_interval_minutes(),
        cron_expression: default_cron_expression(),
    }
}

pub fn default_app_settings() -> AppSettings {
    AppSettings {
        theme: default_theme(),
        language: default_language(),
        toast: default_toast_settings(),
        notification: default_notification_settings(),
    }
}

fn default_toast_settings() -> ToastSettings {
    ToastSettings {
        enabled: true,
        position: default_toast_position(),
        duration_ms: default_toast_duration_ms(),
    }
}

fn default_notification_settings() -> NotificationSettings {
    NotificationSettings {
        enabled: false,
        channel: default_notification_channel(),
        scope: NotificationScope::default(),
        dingtalk: DingtalkNotification::default(),
        telegram: TelegramNotification::default(),
        feishu: FeishuNotification::default(),
    }
}

pub fn safe_domain_filename(domain: &str) -> String {
    domain
        .trim()
        .replace("*.", "wildcard.")
        .replace('*', "wildcard")
        .replace(['/', '\\'], "_")
}

pub fn state_dir_for(profile: &Profile) -> PathBuf {
    PathBuf::from(&profile.paths.state_dir).join(safe_domain_filename(&profile.domain))
}

pub fn work_dir_for(profile: &Profile) -> PathBuf {
    PathBuf::from(&profile.paths.work_dir).join(safe_domain_filename(&profile.domain))
}

fn default_renew() -> RenewConfig {
    RenewConfig {
        days_before_expiry: default_days_before_expiry(),
        force: false,
    }
}
fn default_days_before_expiry() -> i64 {
    30
}
fn default_state_dir() -> String {
    "./state".to_string()
}
fn default_work_dir() -> String {
    "./work".to_string()
}
fn default_log_file() -> String {
    "./logs/ssl-renew.log".to_string()
}
fn default_backup_dir() -> String {
    "D:/cert/backup".to_string()
}
fn default_max_log_size_mb() -> f64 {
    10.0
}
fn default_dns_provider() -> String {
    "manual".to_string()
}
fn default_propagation_timeout() -> u64 {
    600
}
fn default_propagation_interval() -> u64 {
    15
}
fn default_resolvers() -> Vec<String> {
    vec!["223.5.5.5".to_string(), "8.8.8.8".to_string()]
}
fn default_aliyun_key_env() -> String {
    "Ali_Key".to_string()
}
fn default_aliyun_secret_env() -> String {
    "Ali_Secret".to_string()
}
fn default_aliyun_endpoint() -> String {
    "https://alidns.aliyuncs.com/".to_string()
}
fn default_cf_token_env() -> String {
    "CF_Token".to_string()
}
fn default_cf_endpoint() -> String {
    "https://api.cloudflare.com/client/v4".to_string()
}
pub fn default_signer_pipe() -> String {
    r"\\.\pipe\ssl-renew-signer".to_string()
}
fn default_true() -> bool {
    true
}
fn default_nginx_exe() -> String {
    "D:/nginx/nginx.exe".to_string()
}
fn default_nginx_restart_mode() -> String {
    "kill_start".to_string()
}
fn default_nginx_dir() -> String {
    "D:/nginx".to_string()
}
fn default_nginx_image() -> String {
    "nginx.exe".to_string()
}
fn default_monitor_mode() -> String {
    "daily".to_string()
}
fn default_daily_time() -> String {
    "10:00".to_string()
}
fn default_interval_minutes() -> u64 {
    1440
}
fn default_cron_expression() -> String {
    "0 10 * * *".to_string()
}
fn default_theme() -> String {
    "light".to_string()
}
fn default_language() -> String {
    "zh".to_string()
}
fn default_toast_position() -> String {
    "top-right".to_string()
}
fn default_toast_duration_ms() -> u64 {
    3200
}
fn default_notification_channel() -> String {
    "dingtalk".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_domain_matches_python_style() {
        assert_eq!(safe_domain_filename("*.h5por.com"), "wildcard.h5por.com");
    }

    #[test]
    fn profiles_yaml_is_compatible() {
        let text = r#"
current_domain: '*.h5por.com'
profiles:
  '*.h5por.com':
    domain: '*.h5por.com'
    email: admin@h5por.com
    renew:
      days_before_expiry: 8
    paths:
      cert_file: D:/cert/h5por.com.pem
      key_file: D:/cert/h5por.com.key
    dns:
      provider: aliyun
vendor_configs:
  manual: []
monitor:
  enabled: false
"#;
        let mut store: Store = serde_yaml::from_str(text).unwrap();
        normalize_store(&mut store);
        assert_eq!(store.profiles["*.h5por.com"].dns.provider, "aliyun");
        assert_eq!(store.profiles["*.h5por.com"].paths.max_log_size_mb, 10.0);
        assert_eq!(
            store.profiles["*.h5por.com"].nginx.restart_mode,
            "kill_start"
        );
    }

    #[test]
    fn missing_profiles_file_uses_example_domain() {
        let profile = default_profile("*.example.com");
        assert_eq!(profile.domain, "*.example.com");
        assert_eq!(profile.email, "admin@example.com");
        assert!(profile.paths.cert_file.contains("wildcard.example.com"));
    }
}
