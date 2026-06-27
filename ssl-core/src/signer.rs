use crate::config::default_signer_pipe;
use crate::dns::{AliyunDnsProvider, CloudflareDnsProvider, DnsChallengeInfo, DnsProvider};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, bail, Context, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use async_trait::async_trait;
use base64::Engine;
use chrono::{Duration as ChronoDuration, Local, Timelike, Utc};
use hmac::{Hmac, Mac};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use zeroize::Zeroize;

pub const SIGNER_STATE_DIR: &str = "signer-state";
pub const SIGNER_SECRETS_FILE: &str = "signer-secrets.yaml";
pub const PROTECTION_DPAPI_V1: &str = "dpapi_v1";
pub const PROTECTION_PASSPHRASE_DPAPI_V1: &str = "passphrase_dpapi_v1";

const SECURITY_ERROR: &str = "签发程序安全校验未通过";
const AUTH_WINDOW_SECONDS: i64 = 300;
const ARGON2_M_COST: u32 = 19_456;
const ARGON2_T_COST: u32 = 2;
const ARGON2_P_COST: u32 = 1;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignerSecretsFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "default_protection_mode")]
    pub protection_mode: String,
    pub provider: String,
    pub root_domain: String,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    pub ttl: u32,
    #[serde(default = "default_signer_pipe")]
    pub pipe_name: String,
    #[serde(default)]
    pub pipe_sddl: Option<String>,
    pub aliyun: Option<AliyunSignerSecrets>,
    pub cloudflare: Option<CloudflareSignerSecrets>,
    #[serde(default)]
    pub secure: Option<SecureSignerSecrets>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AliyunSignerSecrets {
    pub access_key_id_dpapi: String,
    pub access_key_secret_dpapi: String,
    #[serde(default = "default_aliyun_endpoint")]
    pub endpoint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CloudflareSignerSecrets {
    pub token_dpapi: String,
    #[serde(default = "default_cf_endpoint")]
    pub endpoint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SecureSignerSecrets {
    pub metadata_dpapi: String,
    pub nonce: String,
    pub ciphertext: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SecureMetadata {
    salt: String,
    memory_cost: u32,
    time_cost: u32,
    parallelism: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PlainCredentials {
    provider: String,
    aliyun: Option<PlainAliyunCredentials>,
    cloudflare: Option<PlainCloudflareCredentials>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PlainAliyunCredentials {
    access_key_id: String,
    access_key_secret: String,
    endpoint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PlainCloudflareCredentials {
    token: String,
    endpoint: String,
}

#[derive(Clone, Debug)]
enum RuntimeCredentials {
    Aliyun {
        access_key_id: String,
        access_key_secret: String,
        endpoint: String,
    },
    Cloudflare {
        token: String,
        endpoint: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignerInitRequest {
    pub provider: String,
    pub root_domain: String,
    pub allowed_domains: Vec<String>,
    pub ttl: Option<u32>,
    pub pipe_name: Option<String>,
    pub pipe_sddl: Option<String>,
    #[serde(default)]
    pub protection_mode: Option<String>,
    #[serde(default)]
    pub unlock_passphrase: Option<String>,
    pub aliyun_access_key_id: Option<String>,
    pub aliyun_access_key_secret: Option<String>,
    pub aliyun_endpoint: Option<String>,
    pub cloudflare_token: Option<String>,
    pub cloudflare_endpoint: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignerPresentRequest {
    pub domain: String,
    pub txt_name: String,
    pub rr_name: String,
    pub txt_value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignerRuntimeStatus {
    pub protection_mode: String,
    pub provider: String,
    pub unlocked: bool,
    pub authorized: bool,
    pub root_domain: String,
    pub allowed_domains: Vec<String>,
    pub pipe_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignerResponse {
    pub ok: bool,
    pub message: String,
    #[serde(default)]
    pub status: Option<SignerRuntimeStatus>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum SignerPipeRequest {
    Authorize {
        nonce: String,
        process_name: String,
        proof: String,
    },
    Unlock {
        passphrase: String,
    },
    Lock,
    Status,
    Present {
        request: SignerPresentRequest,
    },
}

pub struct SignerDnsProvider {
    pipe_name: String,
}

impl SignerDnsProvider {
    pub fn new(pipe_name: String) -> Self {
        Self { pipe_name }
    }
}

#[async_trait]
impl DnsProvider for SignerDnsProvider {
    async fn present(&self, challenge: &DnsChallengeInfo) -> Result<()> {
        let response = present_via_pipe(
            &self.pipe_name,
            &SignerPresentRequest {
                domain: challenge.domain.clone(),
                txt_name: challenge.txt_name.clone(),
                rr_name: challenge.rr_name.clone(),
                txt_value: challenge.txt_value.clone(),
            },
        )
        .await?;
        if response.ok {
            Ok(())
        } else {
            bail!("{}", response.message)
        }
    }
}

#[derive(Clone)]
struct SignerRuntime {
    config: SignerSecretsFile,
    unlocked_credentials: Option<RuntimeCredentials>,
    authorized_until: Option<chrono::DateTime<Utc>>,
}

pub fn default_secrets_path() -> PathBuf {
    PathBuf::from(SIGNER_STATE_DIR).join(SIGNER_SECRETS_FILE)
}

pub fn signer_status(path: impl AsRef<Path>) -> Result<String> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(format!("未初始化 signer：{}", path.display()));
    }
    let config = load_config(path)?;
    Ok(format!(
        "signer 已初始化：provider={}, protection_mode={}, root_domain={}, allowed_domains={}, pipe={}",
        config.provider,
        config.protection_mode,
        config.root_domain,
        config.allowed_domains.join(","),
        config.pipe_name
    ))
}

pub fn init_config(
    path: impl AsRef<Path>,
    mut request: SignerInitRequest,
) -> Result<SignerSecretsFile> {
    let provider = normalize_provider(&request.provider);
    let root_domain = clean_domain(&request.root_domain);
    if root_domain.is_empty() {
        bail!("根域名不能为空");
    }
    let allowed_domains = if request.allowed_domains.is_empty() {
        vec![root_domain.clone()]
    } else {
        request
            .allowed_domains
            .iter()
            .map(|item| clean_domain(item))
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
    };
    let ttl = request
        .ttl
        .unwrap_or_else(|| if provider == "cloudflare" { 120 } else { 600 });
    let pipe_name = request.pipe_name.unwrap_or_else(default_signer_pipe);
    let protection_mode = request
        .protection_mode
        .clone()
        .unwrap_or_else(|| PROTECTION_PASSPHRASE_DPAPI_V1.to_string());

    let mut config = match provider.as_str() {
        "aliyun" => {
            let access_key_id =
                required(request.aliyun_access_key_id.take(), "阿里云 AccessKeyId")?;
            let access_key_secret = required(
                request.aliyun_access_key_secret.take(),
                "阿里云 AccessKeySecret",
            )?;
            let endpoint = request
                .aliyun_endpoint
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(default_aliyun_endpoint);
            build_config(
                &provider,
                root_domain,
                allowed_domains,
                ttl,
                pipe_name,
                request.pipe_sddl,
                CredentialsInput::Aliyun {
                    access_key_id,
                    access_key_secret,
                    endpoint,
                },
                &protection_mode,
                request.unlock_passphrase.take(),
            )?
        }
        "cloudflare" => {
            let token = required(request.cloudflare_token.take(), "Cloudflare API Token")?;
            let endpoint = request
                .cloudflare_endpoint
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(default_cf_endpoint);
            build_config(
                &provider,
                root_domain,
                allowed_domains,
                ttl,
                pipe_name,
                request.pipe_sddl,
                CredentialsInput::Cloudflare { token, endpoint },
                &protection_mode,
                request.unlock_passphrase.take(),
            )?
        }
        _ => bail!("signer 暂不支持厂商：{}", request.provider),
    };
    config.version = 1;
    save_config(path, &config)?;
    Ok(config)
}

enum CredentialsInput {
    Aliyun {
        access_key_id: String,
        access_key_secret: String,
        endpoint: String,
    },
    Cloudflare {
        token: String,
        endpoint: String,
    },
}

fn build_config(
    provider: &str,
    root_domain: String,
    allowed_domains: Vec<String>,
    ttl: u32,
    pipe_name: String,
    pipe_sddl: Option<String>,
    credentials: CredentialsInput,
    protection_mode: &str,
    unlock_passphrase: Option<String>,
) -> Result<SignerSecretsFile> {
    let mut config = SignerSecretsFile {
        version: 1,
        protection_mode: protection_mode.to_string(),
        provider: provider.to_string(),
        root_domain,
        allowed_domains,
        ttl,
        pipe_name,
        pipe_sddl: pipe_sddl.filter(|value| !value.trim().is_empty()),
        aliyun: None,
        cloudflare: None,
        secure: None,
    };

    match protection_mode {
        PROTECTION_DPAPI_V1 => match credentials {
            CredentialsInput::Aliyun {
                access_key_id,
                access_key_secret,
                endpoint,
            } => {
                config.aliyun = Some(AliyunSignerSecrets {
                    access_key_id_dpapi: dpapi_protect_to_base64(access_key_id.as_bytes())?,
                    access_key_secret_dpapi: dpapi_protect_to_base64(access_key_secret.as_bytes())?,
                    endpoint,
                });
            }
            CredentialsInput::Cloudflare { token, endpoint } => {
                config.cloudflare = Some(CloudflareSignerSecrets {
                    token_dpapi: dpapi_protect_to_base64(token.as_bytes())?,
                    endpoint,
                });
            }
        },
        PROTECTION_PASSPHRASE_DPAPI_V1 => {
            let mut passphrase = required(unlock_passphrase, "signer 解锁口令")?;
            let plain = match credentials {
                CredentialsInput::Aliyun {
                    access_key_id,
                    access_key_secret,
                    endpoint,
                } => PlainCredentials {
                    provider: "aliyun".to_string(),
                    aliyun: Some(PlainAliyunCredentials {
                        access_key_id,
                        access_key_secret,
                        endpoint,
                    }),
                    cloudflare: None,
                },
                CredentialsInput::Cloudflare { token, endpoint } => PlainCredentials {
                    provider: "cloudflare".to_string(),
                    aliyun: None,
                    cloudflare: Some(PlainCloudflareCredentials { token, endpoint }),
                },
            };
            config.secure = Some(encrypt_secure_credentials(&plain, &passphrase)?);
            passphrase.zeroize();
        }
        other => bail!("未知 signer 保护模式：{other}"),
    }
    Ok(config)
}

pub fn load_config(path: impl AsRef<Path>) -> Result<SignerSecretsFile> {
    let path = path.as_ref();
    let text = fs::read_to_string(path)
        .with_context(|| format!("读取 signer 配置失败：{}", path.display()))?;
    serde_yaml::from_str(&text).context("解析 signer 配置失败")
}

fn save_config(path: impl AsRef<Path>, config: &SignerSecretsFile) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_yaml::to_string(config)?;
    fs::write(path, text).with_context(|| format!("保存 signer 配置失败：{}", path.display()))
}

#[cfg(windows)]
pub async fn serve(path: impl AsRef<Path>) -> Result<()> {
    check_start_environment()?;
    let path = path.as_ref().to_path_buf();
    let config = load_config(&path)?;
    println!("signer 已启动，pipe：{}", config.pipe_name);
    let runtime = Arc::new(Mutex::new(SignerRuntime {
        config,
        unlocked_credentials: None,
        authorized_until: None,
    }));
    loop {
        let config = runtime
            .lock()
            .map_err(|_| anyhow!(SECURITY_ERROR))?
            .config
            .clone();
        let server = create_pipe_server(&config)
            .with_context(|| format!("创建 named pipe 失败：{}", config.pipe_name))?;
        server
            .connect()
            .await
            .context("等待 named pipe 客户端失败")?;
        let runtime = runtime.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_pipe_client(server, runtime).await {
                eprintln!("signer 请求失败：{err:#}");
            }
        });
    }
}

#[cfg(windows)]
fn create_pipe_server(
    config: &SignerSecretsFile,
) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    use tokio::net::windows::named_pipe::ServerOptions;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;

    let options = ServerOptions::new();
    let Some(sddl) = config
        .pipe_sddl
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    else {
        return options.create(&config.pipe_name);
    };
    let mut wide = sddl.encode_utf16().collect::<Vec<_>>();
    wide.push(0);
    let mut descriptor = std::ptr::null_mut();
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let created = unsafe {
        options.create_with_security_attributes_raw(
            &config.pipe_name,
            &mut attrs as *mut SECURITY_ATTRIBUTES as *mut core::ffi::c_void,
        )
    };
    unsafe {
        LocalFree(descriptor);
    }
    created
}

#[cfg(not(windows))]
pub async fn serve(_path: impl AsRef<Path>) -> Result<()> {
    bail!("signer named pipe 仅支持 Windows")
}

async fn handle_pipe_client<S>(mut stream: S, runtime: Arc<Mutex<SignerRuntime>>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request: SignerPipeRequest = read_frame(&mut stream).await?;
    let response = match process_pipe_request(runtime, request).await {
        Ok(response) => response,
        Err(err) => SignerResponse {
            ok: false,
            message: err.to_string(),
            status: None,
        },
    };
    write_frame(&mut stream, &response).await?;
    stream.shutdown().await?;
    Ok(())
}

async fn process_pipe_request(
    runtime: Arc<Mutex<SignerRuntime>>,
    request: SignerPipeRequest,
) -> Result<SignerResponse> {
    match request {
        SignerPipeRequest::Authorize {
            nonce,
            process_name,
            proof,
        } => {
            verify_authorization_knock(&nonce, &process_name, &proof)?;
            let mut runtime = runtime.lock().map_err(|_| anyhow!(SECURITY_ERROR))?;
            runtime.authorized_until =
                Some(Utc::now() + ChronoDuration::seconds(AUTH_WINDOW_SECONDS));
            Ok(SignerResponse {
                ok: true,
                message: "signer 授权会话已建立".to_string(),
                status: Some(runtime_status(&runtime)),
            })
        }
        SignerPipeRequest::Unlock { mut passphrase } => {
            let credentials = {
                let runtime = runtime.lock().map_err(|_| anyhow!(SECURITY_ERROR))?;
                match runtime.config.protection_mode.as_str() {
                    PROTECTION_PASSPHRASE_DPAPI_V1 => {
                        decrypt_secure_credentials(&runtime.config, &passphrase)
                            .map_err(|_| anyhow!("signer 解锁失败"))?
                    }
                    PROTECTION_DPAPI_V1 => legacy_credentials(&runtime.config)?,
                    _ => bail!(SECURITY_ERROR),
                }
            };
            passphrase.zeroize();
            let mut runtime = runtime.lock().map_err(|_| anyhow!(SECURITY_ERROR))?;
            runtime.unlocked_credentials = Some(credentials);
            Ok(SignerResponse {
                ok: true,
                message: "signer 已解锁".to_string(),
                status: Some(runtime_status(&runtime)),
            })
        }
        SignerPipeRequest::Lock => {
            let mut runtime = runtime.lock().map_err(|_| anyhow!(SECURITY_ERROR))?;
            runtime.unlocked_credentials = None;
            runtime.authorized_until = None;
            Ok(SignerResponse {
                ok: true,
                message: "signer 已锁定".to_string(),
                status: Some(runtime_status(&runtime)),
            })
        }
        SignerPipeRequest::Status => {
            let runtime = runtime.lock().map_err(|_| anyhow!(SECURITY_ERROR))?;
            Ok(SignerResponse {
                ok: true,
                message: format!(
                    "signer 状态：{}，授权：{}",
                    if runtime_status(&runtime).unlocked {
                        "已解锁"
                    } else {
                        "未解锁"
                    },
                    if runtime_status(&runtime).authorized {
                        "有效"
                    } else {
                        "无效"
                    }
                ),
                status: Some(runtime_status(&runtime)),
            })
        }
        SignerPipeRequest::Present { request } => present_from_runtime(runtime, &request).await,
    }
}

fn runtime_status(runtime: &SignerRuntime) -> SignerRuntimeStatus {
    SignerRuntimeStatus {
        protection_mode: runtime.config.protection_mode.clone(),
        provider: runtime.config.provider.clone(),
        unlocked: runtime.config.protection_mode == PROTECTION_DPAPI_V1
            || runtime.unlocked_credentials.is_some(),
        authorized: runtime
            .authorized_until
            .map(|until| until > Utc::now())
            .unwrap_or(false),
        root_domain: runtime.config.root_domain.clone(),
        allowed_domains: runtime.config.allowed_domains.clone(),
        pipe_name: runtime.config.pipe_name.clone(),
    }
}

async fn present_from_runtime(
    runtime: Arc<Mutex<SignerRuntime>>,
    request: &SignerPresentRequest,
) -> Result<SignerResponse> {
    let mut flow = FlowGuard::new();
    check_start_environment().map_err(|_| anyhow!(SECURITY_ERROR))?;
    flow.mark(1);
    let (config, credentials) = {
        let runtime = runtime.lock().map_err(|_| anyhow!(SECURITY_ERROR))?;
        if runtime
            .authorized_until
            .map(|until| until <= Utc::now())
            .unwrap_or(true)
        {
            bail!("signer 未授权，请先由主程序建立授权会话");
        }
        flow.mark(2);
        let credentials = match runtime.config.protection_mode.as_str() {
            PROTECTION_PASSPHRASE_DPAPI_V1 => runtime
                .unlocked_credentials
                .clone()
                .ok_or_else(|| anyhow!("signer 未解锁，请先输入口令解锁"))?,
            PROTECTION_DPAPI_V1 => legacy_credentials(&runtime.config)?,
            _ => bail!(SECURITY_ERROR),
        };
        flow.mark(3);
        (runtime.config.clone(), credentials)
    };
    validate_request_with_flow(&config, request, &mut flow)?;
    flow.mark(5);
    present_with_credentials(&config, &credentials, request).await?;
    flow.mark(6);
    flow.ensure_complete()?;
    let message = format!(
        "{} signer 已写入 TXT：{}",
        Local::now().format("%Y-%m-%d %H:%M:%S"),
        request.txt_name
    );
    println!("{message}");
    let status = {
        let runtime = runtime.lock().map_err(|_| anyhow!(SECURITY_ERROR))?;
        runtime_status(&runtime)
    };
    Ok(SignerResponse {
        ok: true,
        message,
        status: Some(status),
    })
}

async fn present_with_credentials(
    config: &SignerSecretsFile,
    credentials: &RuntimeCredentials,
    request: &SignerPresentRequest,
) -> Result<()> {
    let challenge = DnsChallengeInfo {
        domain: request.domain.clone(),
        txt_name: request.txt_name.clone(),
        rr_name: request.rr_name.clone(),
        txt_value: request.txt_value.clone(),
    };
    match credentials {
        RuntimeCredentials::Aliyun {
            access_key_id,
            access_key_secret,
            endpoint,
        } => {
            let provider = AliyunDnsProvider::new(
                access_key_id.clone(),
                access_key_secret.clone(),
                config.root_domain.clone(),
                endpoint.clone(),
            );
            provider.present(&challenge).await?;
        }
        RuntimeCredentials::Cloudflare { token, endpoint } => {
            let provider = CloudflareDnsProvider::new(
                token.clone(),
                config.root_domain.clone(),
                endpoint.clone(),
            );
            provider.present(&challenge).await?;
        }
    }
    Ok(())
}

pub async fn present_from_config(
    path: impl AsRef<Path>,
    request: &SignerPresentRequest,
) -> Result<String> {
    let config = load_config(path)?;
    validate_request(&config, request)?;
    let credentials = legacy_credentials(&config)?;
    present_with_credentials(&config, &credentials, request).await?;
    Ok(format!("signer 已写入 TXT：{}", request.txt_name))
}

#[cfg(windows)]
pub async fn send_pipe_request(
    pipe_name: &str,
    request: &SignerPipeRequest,
) -> Result<SignerResponse> {
    use tokio::net::windows::named_pipe::ClientOptions;

    let mut client = ClientOptions::new()
        .open(pipe_name)
        .with_context(|| format!("连接 signer named pipe 失败：{pipe_name}"))?;
    write_frame(&mut client, request).await?;
    let response = read_frame(&mut client).await?;
    Ok(response)
}

#[cfg(not(windows))]
pub async fn send_pipe_request(
    _pipe_name: &str,
    _request: &SignerPipeRequest,
) -> Result<SignerResponse> {
    bail!("signer named pipe 仅支持 Windows")
}

pub async fn authorize_via_pipe(pipe_name: &str) -> Result<SignerResponse> {
    let nonce = uuid::Uuid::new_v4().to_string();
    let process_name = current_process_name();
    let proof = authorization_proof(&nonce, &process_name)?;
    send_pipe_request(
        pipe_name,
        &SignerPipeRequest::Authorize {
            nonce,
            process_name,
            proof,
        },
    )
    .await
}

pub async fn unlock_via_pipe(pipe_name: &str, passphrase: String) -> Result<SignerResponse> {
    send_pipe_request(pipe_name, &SignerPipeRequest::Unlock { passphrase }).await
}

pub async fn lock_via_pipe(pipe_name: &str) -> Result<SignerResponse> {
    send_pipe_request(pipe_name, &SignerPipeRequest::Lock).await
}

pub async fn status_via_pipe(pipe_name: &str) -> Result<SignerResponse> {
    send_pipe_request(pipe_name, &SignerPipeRequest::Status).await
}

pub async fn present_via_pipe(
    pipe_name: &str,
    request: &SignerPresentRequest,
) -> Result<SignerResponse> {
    let auth = authorize_via_pipe(pipe_name).await?;
    if !auth.ok {
        return Ok(auth);
    }
    send_pipe_request(
        pipe_name,
        &SignerPipeRequest::Present {
            request: request.clone(),
        },
    )
    .await
}

fn legacy_credentials(config: &SignerSecretsFile) -> Result<RuntimeCredentials> {
    match config.provider.as_str() {
        "aliyun" => {
            let secrets = config
                .aliyun
                .as_ref()
                .ok_or_else(|| anyhow!("缺少阿里云 signer 配置"))?;
            Ok(RuntimeCredentials::Aliyun {
                access_key_id: dpapi_unprotect_to_string(&secrets.access_key_id_dpapi)?,
                access_key_secret: dpapi_unprotect_to_string(&secrets.access_key_secret_dpapi)?,
                endpoint: secrets.endpoint.clone(),
            })
        }
        "cloudflare" => {
            let secrets = config
                .cloudflare
                .as_ref()
                .ok_or_else(|| anyhow!("缺少 Cloudflare signer 配置"))?;
            Ok(RuntimeCredentials::Cloudflare {
                token: dpapi_unprotect_to_string(&secrets.token_dpapi)?,
                endpoint: secrets.endpoint.clone(),
            })
        }
        _ => bail!("signer 不支持厂商：{}", config.provider),
    }
}

fn encrypt_secure_credentials(
    plain: &PlainCredentials,
    passphrase: &str,
) -> Result<SecureSignerSecrets> {
    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce_bytes);
    let metadata = SecureMetadata {
        salt: base64::engine::general_purpose::STANDARD.encode(salt),
        memory_cost: ARGON2_M_COST,
        time_cost: ARGON2_T_COST,
        parallelism: ARGON2_P_COST,
    };
    let mut key = derive_passphrase_key(passphrase, &metadata)?;
    let cipher = Aes256Gcm::new_from_slice(&key)?;
    let plaintext = serde_json::to_vec(plain)?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
        .map_err(|_| anyhow!("signer 加密失败"))?;
    key.zeroize();
    let metadata_text = serde_json::to_vec(&metadata)?;
    Ok(SecureSignerSecrets {
        metadata_dpapi: dpapi_protect_to_base64(&metadata_text)?,
        nonce: base64::engine::general_purpose::STANDARD.encode(nonce_bytes),
        ciphertext: base64::engine::general_purpose::STANDARD.encode(ciphertext),
    })
}

fn decrypt_secure_credentials(
    config: &SignerSecretsFile,
    passphrase: &str,
) -> Result<RuntimeCredentials> {
    let secure = config
        .secure
        .as_ref()
        .ok_or_else(|| anyhow!("缺少高安全 signer 密文配置"))?;
    let metadata_text = dpapi_unprotect_to_string(&secure.metadata_dpapi)?;
    let metadata: SecureMetadata = serde_json::from_str(&metadata_text)?;
    let nonce = base64::engine::general_purpose::STANDARD
        .decode(&secure.nonce)
        .context("signer nonce 不是合法 base64")?;
    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(&secure.ciphertext)
        .context("signer 密文不是合法 base64")?;
    let mut key = derive_passphrase_key(passphrase, &metadata)?;
    let cipher = Aes256Gcm::new_from_slice(&key)?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| anyhow!("signer 解锁失败"))?;
    key.zeroize();
    let plain: PlainCredentials = serde_json::from_slice(&plaintext)?;
    match plain.provider.as_str() {
        "aliyun" => {
            let aliyun = plain
                .aliyun
                .ok_or_else(|| anyhow!("缺少阿里云高安全凭据"))?;
            Ok(RuntimeCredentials::Aliyun {
                access_key_id: aliyun.access_key_id,
                access_key_secret: aliyun.access_key_secret,
                endpoint: aliyun.endpoint,
            })
        }
        "cloudflare" => {
            let cloudflare = plain
                .cloudflare
                .ok_or_else(|| anyhow!("缺少 Cloudflare 高安全凭据"))?;
            Ok(RuntimeCredentials::Cloudflare {
                token: cloudflare.token,
                endpoint: cloudflare.endpoint,
            })
        }
        _ => bail!(SECURITY_ERROR),
    }
}

fn derive_passphrase_key(passphrase: &str, metadata: &SecureMetadata) -> Result<[u8; 32]> {
    let salt = base64::engine::general_purpose::STANDARD
        .decode(&metadata.salt)
        .context("signer salt 不是合法 base64")?;
    let params = Params::new(
        metadata.memory_cost,
        metadata.time_cost,
        metadata.parallelism,
        Some(32),
    )
    .map_err(|err| anyhow!("signer KDF 参数无效：{err}"))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(passphrase.as_bytes(), &salt, &mut key)
        .map_err(|err| anyhow!("signer 口令派生失败：{err}"))?;
    Ok(key)
}

fn validate_request(config: &SignerSecretsFile, request: &SignerPresentRequest) -> Result<()> {
    let mut flow = FlowGuard::new();
    validate_request_with_flow(config, request, &mut flow)
}

fn validate_request_with_flow(
    config: &SignerSecretsFile,
    request: &SignerPresentRequest,
    flow: &mut FlowGuard,
) -> Result<()> {
    let txt_name = request
        .txt_name
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if !txt_name.starts_with("_acme-challenge.") {
        bail!("signer 只允许写 _acme-challenge TXT 记录");
    }
    let owner = txt_name.trim_start_matches("_acme-challenge.");
    let allowed = config
        .allowed_domains
        .iter()
        .map(|item| clean_domain(item).to_ascii_lowercase())
        .any(|domain| owner == domain);
    if !allowed {
        bail!("TXT 名称不在 signer 允许域名内：{}", request.txt_name);
    }
    let expected_rr_name = expected_rr_name(config, &txt_name)?;
    if request.rr_name.trim().to_ascii_lowercase() != expected_rr_name {
        bail!("RR 主机记录与 TXT 名称不匹配，期望：{}", expected_rr_name);
    }
    validate_txt_value(&request.txt_value)?;
    flow.mark(4);
    Ok(())
}

fn expected_rr_name(config: &SignerSecretsFile, txt_name: &str) -> Result<String> {
    let root_domain = clean_domain(&config.root_domain).to_ascii_lowercase();
    let suffix = format!(".{root_domain}");
    txt_name
        .strip_suffix(&suffix)
        .map(|item| item.to_string())
        .ok_or_else(|| anyhow!("TXT 名称不属于 signer 根域名：{}", config.root_domain))
}

fn validate_txt_value(value: &str) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.len() < 20 || trimmed.len() > 128 {
        bail!("TXT 值长度不符合 ACME DNS-01 常见范围");
    }
    if !trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("TXT 值包含非法字符");
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct FlowGuard {
    value: u64,
}

impl FlowGuard {
    fn new() -> Self {
        Self {
            value: 0x4d53_5352_4752_4431,
        }
    }

    fn mark(&mut self, value: u64) {
        self.value = self.value.rotate_left(7)
            ^ value.wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ 0xa5a5_5a5a_c3c3_3c3c;
    }

    fn ensure_complete(&self) -> Result<()> {
        let mut expected = FlowGuard::new();
        for value in [1, 2, 3, 4, 5, 6] {
            expected.mark(value);
        }
        if self.value != expected.value {
            bail!(SECURITY_ERROR);
        }
        Ok(())
    }
}

fn verify_authorization_knock(nonce: &str, process_name: &str, proof: &str) -> Result<()> {
    if nonce.len() < 16 || nonce.len() > 128 {
        bail!(SECURITY_ERROR);
    }
    if !is_allowed_client_process(process_name) || !client_process_is_running(process_name) {
        bail!(SECURITY_ERROR);
    }
    let expected = authorization_proof(nonce, process_name)?;
    if expected != proof {
        bail!(SECURITY_ERROR);
    }
    Ok(())
}

fn authorization_proof(nonce: &str, process_name: &str) -> Result<String> {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(&knock_key())?;
    mac.update(nonce.as_bytes());
    mac.update(b"|");
    mac.update(process_name.as_bytes());
    Ok(base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes()))
}

fn knock_key() -> Vec<u8> {
    [
        0x2bu8, 0x18, 0x57, 0x7c, 0x01, 0x0c, 0x55, 0x60, 0x2e, 0x18, 0x45, 0x68, 0x38, 0x0d, 0x45,
        0x68, 0x2b, 0x12, 0x5b, 0x60, 0x35, 0x18, 0x58, 0x7c,
    ]
    .iter()
    .map(|value| value ^ 0x39)
    .collect()
}

fn current_process_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|item| item.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn is_allowed_client_process(process_name: &str) -> bool {
    let name = process_name.trim().to_ascii_lowercase();
    matches!(
        name.as_str(),
        "ssl证书自动续期.exe" | "ssl-renew-cli.exe" | "ssl-renew-gui.exe"
    )
}

fn client_process_is_running(process_name: &str) -> bool {
    #[cfg(windows)]
    {
        let Ok(output) = std::process::Command::new("tasklist")
            .args(["/FI", &format!("IMAGENAME eq {process_name}"), "/NH"])
            .output()
        else {
            return false;
        };
        let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
        text.contains(&process_name.to_ascii_lowercase())
    }
    #[cfg(not(windows))]
    {
        let _ = process_name;
        true
    }
}

fn check_start_environment() -> Result<()> {
    let now = Local::now();
    if now.offset().local_minus_utc() != 8 * 3600 {
        bail!("signer 仅允许在东八区启动");
    }
    if now.hour() < 6 {
        bail!("signer 仅允许在 06:00 到 24:00 之间启动");
    }
    Ok(())
}

async fn read_frame<R, T>(reader: &mut R) -> Result<T>
where
    R: AsyncRead + Unpin,
    T: for<'de> Deserialize<'de>,
{
    let len = reader.read_u32_le().await? as usize;
    if len > 64 * 1024 {
        bail!("signer 请求过大");
    }
    let mut bytes = vec![0u8; len];
    reader.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).context("解析 signer 请求失败")
}

async fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let bytes = serde_json::to_vec(value)?;
    writer.write_u32_le(bytes.len() as u32).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

fn required(value: Option<String>, label: &str) -> Result<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .ok_or_else(|| anyhow!("{label} 不能为空"))
}

fn normalize_provider(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "aliyun" | "ali" | "阿里云" => "aliyun".to_string(),
        "cloudflare" | "cf" => "cloudflare".to_string(),
        other => other.to_string(),
    }
}

fn clean_domain(value: &str) -> String {
    value
        .trim()
        .trim_end_matches('.')
        .trim_start_matches("*.")
        .to_string()
}

fn default_version() -> u32 {
    1
}

fn default_protection_mode() -> String {
    PROTECTION_DPAPI_V1.to_string()
}

fn default_aliyun_endpoint() -> String {
    "https://alidns.aliyuncs.com/".to_string()
}

fn default_cf_endpoint() -> String {
    "https://api.cloudflare.com/client/v4".to_string()
}

#[cfg(windows)]
fn dpapi_protect_to_base64(data: &[u8]) -> Result<String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = CryptProtectData(
            &input,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        );
        if ok == 0 {
            bail!("DPAPI 加密失败");
        }
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as *mut core::ffi::c_void);
        Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
    }
}

#[cfg(not(windows))]
fn dpapi_protect_to_base64(_data: &[u8]) -> Result<String> {
    bail!("DPAPI 仅支持 Windows")
}

#[cfg(windows)]
fn dpapi_unprotect_to_string(value: &str) -> Result<String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let encrypted = base64::engine::general_purpose::STANDARD
        .decode(value)
        .context("DPAPI 密文不是合法 base64")?;
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: encrypted.len() as u32,
            pbData: encrypted.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        );
        if ok == 0 {
            bail!("DPAPI 解密失败：当前 Windows 用户不匹配、机器不匹配、密文损坏或权限不足");
        }
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as *mut core::ffi::c_void);
        String::from_utf8(bytes).context("DPAPI 解密结果不是 UTF-8")
    }
}

#[cfg(not(windows))]
fn dpapi_unprotect_to_string(_value: &str) -> Result<String> {
    bail!("DPAPI 仅支持 Windows")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> SignerSecretsFile {
        SignerSecretsFile {
            version: 1,
            protection_mode: PROTECTION_DPAPI_V1.to_string(),
            provider: "aliyun".to_string(),
            root_domain: "h5por.com".to_string(),
            allowed_domains: vec!["h5por.com".to_string()],
            ttl: 600,
            pipe_name: default_signer_pipe(),
            pipe_sddl: None,
            aliyun: None,
            cloudflare: None,
            secure: None,
        }
    }

    #[test]
    fn signer_allows_only_acme_txt_for_allowed_domain() {
        validate_request(
            &test_config(),
            &SignerPresentRequest {
                domain: "*.h5por.com".to_string(),
                txt_name: "_acme-challenge.h5por.com".to_string(),
                rr_name: "_acme-challenge".to_string(),
                txt_value: "GDl4YTJXT3_xUwYpdPSrDyzj4BazOMC9vulEyapX2cY".to_string(),
            },
        )
        .unwrap();
    }

    #[test]
    fn signer_rejects_non_acme_name() {
        let err = validate_request(
            &test_config(),
            &SignerPresentRequest {
                domain: "*.h5por.com".to_string(),
                txt_name: "www.h5por.com".to_string(),
                rr_name: "www".to_string(),
                txt_value: "GDl4YTJXT3_xUwYpdPSrDyzj4BazOMC9vulEyapX2cY".to_string(),
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("_acme-challenge"));
    }

    #[test]
    fn signer_rejects_other_domain() {
        let err = validate_request(
            &test_config(),
            &SignerPresentRequest {
                domain: "*.evil.com".to_string(),
                txt_name: "_acme-challenge.evil.com".to_string(),
                rr_name: "_acme-challenge".to_string(),
                txt_value: "GDl4YTJXT3_xUwYpdPSrDyzj4BazOMC9vulEyapX2cY".to_string(),
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("允许域名"));
    }

    #[test]
    fn signer_rejects_mismatched_rr_name() {
        let err = validate_request(
            &test_config(),
            &SignerPresentRequest {
                domain: "*.h5por.com".to_string(),
                txt_name: "_acme-challenge.h5por.com".to_string(),
                rr_name: "_acme-challenge.other".to_string(),
                txt_value: "GDl4YTJXT3_xUwYpdPSrDyzj4BazOMC9vulEyapX2cY".to_string(),
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("不匹配"));
    }

    #[test]
    fn flow_guard_accepts_complete_path() {
        let mut flow = FlowGuard::new();
        for value in [1, 2, 3, 4, 5, 6] {
            flow.mark(value);
        }
        flow.ensure_complete().unwrap();
    }

    #[test]
    fn flow_guard_rejects_missing_path() {
        let mut flow = FlowGuard::new();
        for value in [1, 2, 4, 5, 6] {
            flow.mark(value);
        }
        assert!(flow.ensure_complete().is_err());
    }
}
