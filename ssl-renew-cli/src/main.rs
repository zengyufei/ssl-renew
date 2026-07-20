use anyhow::{anyhow, Result};
use clap::{Args, Parser, Subcommand};
use ssl_core::config::{
    default_profile, load_store, profiles_path, resolve_profile_environment_group, save_store,
    DnsProviderKind, EnvGroupEntry, EnvironmentGroup, Profile,
};
use ssl_core::monitor::{next_monitor_run, selected_profiles};
use ssl_core::signer::{
    authorize_via_pipe, default_secrets_path, init_config, lock_via_pipe, present_via_pipe,
    signer_status, status_via_pipe, unlock_via_pipe, SignerInitRequest, SignerPresentRequest,
};
use ssl_core::workflow;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "ssl-renew-cli", version, about = "SSL证书自动续期 CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Profile(ProfileCommand),
    #[command(name = "env-group", visible_alias = "vendor")]
    EnvGroup(EnvGroupCommand),
    Signer(SignerCommand),
    Check(DomainArgs),
    Order(OrderArgs),
    DnsCheck(DomainArgs),
    Issue(DomainArgs),
    Restart(DomainArgs),
    Renew(OrderArgs),
    Monitor,
}

#[derive(Args)]
struct DomainArgs {
    #[arg(long)]
    domain: Option<String>,
}

#[derive(Args)]
struct OrderArgs {
    #[arg(long)]
    domain: Option<String>,
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct ProfileCommand {
    #[command(subcommand)]
    command: ProfileSubcommand,
}

#[derive(Subcommand)]
enum ProfileSubcommand {
    List,
    Show(DomainArgs),
    Add { domain: String },
    Delete { domain: String },
    Set(ProfileSetArgs),
}

#[derive(Args)]
struct ProfileSetArgs {
    #[arg(long)]
    domain: String,
    #[arg(long)]
    new_domain: Option<String>,
    #[arg(long)]
    email: Option<String>,
    #[arg(long)]
    cert_file: Option<String>,
    #[arg(long)]
    key_file: Option<String>,
    #[arg(long)]
    days_before_expiry: Option<i64>,
    #[arg(long)]
    dns_provider: Option<String>,
    #[arg(long)]
    env_group: Option<String>,
    #[arg(long)]
    nginx_enabled: Option<bool>,
    #[arg(long)]
    nginx_restart_mode: Option<String>,
    #[arg(long)]
    nginx_exe: Option<String>,
    #[arg(long)]
    nginx_workdir: Option<String>,
    #[arg(long)]
    signer_pipe: Option<String>,
    #[arg(long)]
    log_file: Option<String>,
    #[arg(long)]
    max_log_size_mb: Option<f64>,
}

#[derive(Args)]
struct EnvGroupCommand {
    #[command(subcommand)]
    command: EnvGroupSubcommand,
}

#[derive(Args)]
struct SignerCommand {
    #[command(subcommand)]
    command: SignerSubcommand,
}

#[derive(Subcommand)]
enum SignerSubcommand {
    Init(SignerInitArgs),
    Status {
        #[arg(long)]
        secrets: Option<PathBuf>,
        #[arg(long)]
        pipe_name: Option<String>,
    },
    Unlock(SignerUnlockArgs),
    Lock(SignerPipeArgs),
    AuthorizeTest(SignerPipeArgs),
    TestPresent(SignerTestPresentArgs),
}

#[derive(Args)]
struct SignerInitArgs {
    #[arg(long)]
    provider: String,
    #[arg(long)]
    root_domain: String,
    #[arg(long, value_delimiter = ',')]
    allowed_domains: Vec<String>,
    #[arg(long)]
    ttl: Option<u32>,
    #[arg(long)]
    pipe_name: Option<String>,
    #[arg(long)]
    pipe_sddl: Option<String>,
    #[arg(long, default_value = "passphrase_dpapi_v1")]
    protection_mode: String,
    #[arg(long)]
    unlock_passphrase: Option<String>,
    #[arg(long)]
    aliyun_access_key_id: Option<String>,
    #[arg(long)]
    aliyun_access_key_secret: Option<String>,
    #[arg(long)]
    aliyun_endpoint: Option<String>,
    #[arg(long)]
    cloudflare_token: Option<String>,
    #[arg(long)]
    cloudflare_endpoint: Option<String>,
    #[arg(long)]
    secrets: Option<PathBuf>,
}

#[derive(Args)]
struct SignerPipeArgs {
    #[arg(long, default_value = "\\\\.\\pipe\\ssl-renew-signer")]
    pipe_name: String,
}

#[derive(Args)]
struct SignerUnlockArgs {
    #[arg(long, default_value = "\\\\.\\pipe\\ssl-renew-signer")]
    pipe_name: String,
    #[arg(long)]
    passphrase: String,
}

#[derive(Args)]
struct SignerTestPresentArgs {
    #[arg(long)]
    pipe_name: String,
    #[arg(long)]
    domain: String,
    #[arg(long)]
    txt_name: String,
    #[arg(long)]
    rr_name: String,
    #[arg(long)]
    txt_value: String,
}

#[derive(Subcommand)]
enum EnvGroupSubcommand {
    List,
    Add {
        name: String,
        #[arg(long)]
        id: Option<String>,
    },
    Rename {
        id: String,
        name: String,
    },
    Delete {
        id: String,
    },
    AddEntry {
        group_id: String,
        alias: String,
        env_name: String,
    },
    RemoveEntry {
        group_id: String,
        alias: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Profile(cmd) => profile_command(cmd)?,
        Commands::EnvGroup(cmd) => env_group_command(cmd)?,
        Commands::Signer(cmd) => signer_command(cmd).await?,
        Commands::Check(args) => {
            let profile = profile_from_args(args.domain)?;
            let status = workflow::check_certificate(&profile, false).await?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        Commands::Order(args) => {
            let profile = profile_from_args(args.domain.clone())?;
            if !args.force {
                let status = workflow::check_certificate(&profile, false).await?;
                if !status.should_renew {
                    println!("当前证书暂不需要续期；仍要创建订单请加 --force");
                    return Ok(());
                }
            }
            let profile = runtime_profile_from_args(args.domain)?;
            let runtime = workflow::create_order_prepare_dns(&profile).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&runtime.session.challenges)?
            );
        }
        Commands::DnsCheck(args) => {
            let profile = profile_from_args(args.domain)?;
            let challenges = workflow::load_saved_challenges(&profile)?;
            if workflow::dns_records_visible(&profile, &challenges).await? {
                println!("DNS TXT 记录已生效");
            } else {
                println!("DNS TXT 记录尚未生效");
            }
        }
        Commands::Issue(args) => {
            let profile = profile_from_args(args.domain)?;
            workflow::issue_certificate(&profile).await?;
            println!("证书已签发并保存");
        }
        Commands::Restart(args) => {
            let profile = profile_from_args(args.domain)?;
            workflow::restart_nginx_for_profile(&profile).await?;
            println!("Nginx 重启步骤已执行");
        }
        Commands::Renew(args) => {
            let profile = runtime_profile_from_args(args.domain)?;
            let outcome = workflow::renew_profile(&profile, args.force).await?;
            println!("{}", outcome.message);
        }
        Commands::Monitor => monitor_foreground().await?,
    }
    Ok(())
}

fn profile_command(cmd: ProfileCommand) -> Result<()> {
    let path = profiles_path();
    let mut store = load_store(&path)?;
    match cmd.command {
        ProfileSubcommand::List => {
            for domain in store.profiles.keys() {
                println!("{domain}");
            }
        }
        ProfileSubcommand::Show(args) => {
            let domain = args.domain.unwrap_or_else(|| store.current_domain.clone());
            let profile = store
                .profiles
                .get(&domain)
                .ok_or_else(|| anyhow!("找不到配置：{domain}"))?;
            println!("{}", serde_json::to_string_pretty(profile)?);
        }
        ProfileSubcommand::Add { domain } => {
            let profile = default_profile(&domain);
            store.current_domain = domain.clone();
            store.profiles.insert(domain, profile);
            save_store(&path, &store)?;
            println!("已新增配置");
        }
        ProfileSubcommand::Delete { domain } => {
            store.profiles.remove(&domain);
            if store.current_domain == domain {
                store.current_domain = store.profiles.keys().next().cloned().unwrap_or_default();
            }
            save_store(&path, &store)?;
            println!("已删除配置");
        }
        ProfileSubcommand::Set(args) => {
            let old_domain = args.domain.clone();
            let mut profile = store
                .profiles
                .remove(&old_domain)
                .ok_or_else(|| anyhow!("找不到配置：{old_domain}"))?;
            apply_profile_set(&mut profile, args);
            let new_domain = profile.domain.clone();
            if store.current_domain == old_domain {
                store.current_domain = new_domain.clone();
            }
            store.profiles.insert(new_domain, profile);
            save_store(&path, &store)?;
            println!("已保存配置");
        }
    }
    Ok(())
}

fn apply_profile_set(profile: &mut Profile, args: ProfileSetArgs) {
    if let Some(value) = args.new_domain {
        profile.domain = value;
    }
    if let Some(value) = args.email {
        profile.email = value;
    }
    if let Some(value) = args.cert_file {
        profile.paths.cert_file = value;
    }
    if let Some(value) = args.key_file {
        profile.paths.key_file = value;
    }
    if let Some(value) = args.days_before_expiry {
        profile.renew.days_before_expiry = value;
    }
    if let Some(value) = args.dns_provider {
        profile.dns.provider = DnsProviderKind::from_value(&value).as_str().to_string();
    }
    if let Some(value) = args.env_group {
        profile.dns.env_group_id = (!value.trim().is_empty()).then_some(value);
    }
    if let Some(value) = args.nginx_enabled {
        profile.nginx.enabled = value;
    }
    if let Some(value) = args.nginx_restart_mode {
        profile.nginx.restart_mode = match value.trim().to_ascii_lowercase().as_str() {
            "reload" => "reload".to_string(),
            _ => "kill_start".to_string(),
        };
    }
    if let Some(value) = args.nginx_exe {
        profile.nginx.exe_path = value;
    }
    if let Some(value) = args.nginx_workdir {
        profile.nginx.working_dir = value;
    }
    if let Some(value) = args.signer_pipe {
        profile.dns.signer.pipe_name = value;
    }
    if let Some(value) = args.log_file {
        profile.paths.log_file = value;
    }
    if let Some(value) = args.max_log_size_mb {
        profile.paths.max_log_size_mb = value;
    }
}

fn env_group_command(cmd: EnvGroupCommand) -> Result<()> {
    let path = profiles_path();
    let mut store = load_store(&path)?;
    match cmd.command {
        EnvGroupSubcommand::List => {
            println!("{}", serde_json::to_string_pretty(&store.env_groups)?);
        }
        EnvGroupSubcommand::Add { name, id } => {
            let id = id.unwrap_or_else(|| {
                format!(
                    "env-group-{}",
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
                )
            });
            if store.env_groups.contains_key(&id) {
                return Err(anyhow!("环境变量组 ID 已存在：{id}"));
            }
            store.env_groups.insert(
                id,
                EnvironmentGroup {
                    name,
                    entries: vec![],
                },
            );
            save_store(&path, &store)?;
            println!("已新增环境变量组");
        }
        EnvGroupSubcommand::Rename { id, name } => {
            let group = store
                .env_groups
                .get_mut(&id)
                .ok_or_else(|| anyhow!("找不到环境变量组：{id}"))?;
            group.name = name;
            save_store(&path, &store)?;
            println!("已重命名环境变量组");
        }
        EnvGroupSubcommand::Delete { id } => {
            let referenced_by = store
                .profiles
                .values()
                .find(|profile| profile.dns.env_group_id.as_deref() == Some(id.as_str()))
                .map(|profile| profile.domain.clone());
            if let Some(domain) = referenced_by {
                return Err(anyhow!(
                    "环境变量组 {id} 正被域名配置 {domain} 引用，不能删除"
                ));
            }
            if store.env_groups.remove(&id).is_none() {
                return Err(anyhow!("找不到环境变量组：{id}"));
            }
            save_store(&path, &store)?;
            println!("已删除环境变量组");
        }
        EnvGroupSubcommand::AddEntry {
            group_id,
            alias,
            env_name,
        } => {
            store
                .env_groups
                .get_mut(&group_id)
                .ok_or_else(|| anyhow!("找不到环境变量组：{group_id}"))?
                .entries
                .push(EnvGroupEntry { alias, env_name });
            save_store(&path, &store)?;
            println!("已添加环境变量名称");
        }
        EnvGroupSubcommand::RemoveEntry { group_id, alias } => {
            let group = store
                .env_groups
                .get_mut(&group_id)
                .ok_or_else(|| anyhow!("找不到环境变量组：{group_id}"))?;
            group.entries.retain(|item| item.alias != alias);
            save_store(&path, &store)?;
            println!("已删除环境变量名称");
        }
    }
    Ok(())
}

fn profile_from_args(domain: Option<String>) -> Result<Profile> {
    let store = load_store(profiles_path())?;
    let domain = domain.unwrap_or_else(|| store.current_domain.clone());
    store
        .profiles
        .get(&domain)
        .cloned()
        .ok_or_else(|| anyhow!("找不到配置：{domain}"))
}

fn runtime_profile_from_args(domain: Option<String>) -> Result<Profile> {
    let store = load_store(profiles_path())?;
    let domain = domain.unwrap_or_else(|| store.current_domain.clone());
    let profile = store
        .profiles
        .get(&domain)
        .ok_or_else(|| anyhow!("找不到配置：{domain}"))?;
    resolve_profile_environment_group(&store, &profile)
}

async fn signer_command(cmd: SignerCommand) -> Result<()> {
    match cmd.command {
        SignerSubcommand::Init(args) => {
            let path = args.secrets.unwrap_or_else(default_secrets_path);
            init_config(
                &path,
                SignerInitRequest {
                    provider: args.provider,
                    root_domain: args.root_domain,
                    allowed_domains: args.allowed_domains,
                    ttl: args.ttl,
                    pipe_name: args.pipe_name,
                    pipe_sddl: args.pipe_sddl,
                    protection_mode: Some(args.protection_mode),
                    unlock_passphrase: args.unlock_passphrase,
                    aliyun_access_key_id: args.aliyun_access_key_id,
                    aliyun_access_key_secret: args.aliyun_access_key_secret,
                    aliyun_endpoint: args.aliyun_endpoint,
                    cloudflare_token: args.cloudflare_token,
                    cloudflare_endpoint: args.cloudflare_endpoint,
                },
            )?;
            println!("signer 初始化完成：{}", path.display());
        }
        SignerSubcommand::Status { secrets, pipe_name } => {
            if let Some(pipe_name) = pipe_name {
                let response = status_via_pipe(&pipe_name).await?;
                println!("{}", response.message);
                if let Some(status) = response.status {
                    println!("{}", serde_json::to_string_pretty(&status)?);
                }
                if !response.ok {
                    return Err(anyhow!(response.message));
                }
            } else {
                println!(
                    "{}",
                    signer_status(secrets.unwrap_or_else(default_secrets_path))?
                );
            }
        }
        SignerSubcommand::Unlock(args) => {
            let response = unlock_via_pipe(&args.pipe_name, args.passphrase).await?;
            println!("{}", response.message);
            if !response.ok {
                return Err(anyhow!(response.message));
            }
        }
        SignerSubcommand::Lock(args) => {
            let response = lock_via_pipe(&args.pipe_name).await?;
            println!("{}", response.message);
            if !response.ok {
                return Err(anyhow!(response.message));
            }
        }
        SignerSubcommand::AuthorizeTest(args) => {
            let response = authorize_via_pipe(&args.pipe_name).await?;
            println!("{}", response.message);
            if !response.ok {
                return Err(anyhow!(response.message));
            }
        }
        SignerSubcommand::TestPresent(args) => {
            let response = present_via_pipe(
                &args.pipe_name,
                &SignerPresentRequest {
                    domain: args.domain,
                    txt_name: args.txt_name,
                    rr_name: args.rr_name,
                    txt_value: args.txt_value,
                },
            )
            .await?;
            println!("{}", response.message);
            if !response.ok {
                return Err(anyhow!(response.message));
            }
        }
    }
    Ok(())
}

async fn monitor_foreground() -> Result<()> {
    loop {
        let store = load_store(profiles_path())?;
        let next = next_monitor_run(&store.monitor)?;
        println!("下一次监控时间：{}", next.format("%Y-%m-%d %H:%M:%S"));
        let now = chrono::Local::now();
        let wait_ms = (next - now).num_milliseconds().max(1000) as u64;
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
        let store = load_store(profiles_path())?;
        for (domain, profile) in selected_profiles(&store.monitor, &store.profiles) {
            println!("开始监控配置：{domain}");
            if DnsProviderKind::from_value(&profile.dns.provider) == DnsProviderKind::Manual {
                println!("手动DNS 无法无人值守续期，已跳过：{domain}");
                continue;
            }
            let runtime_profile = match resolve_profile_environment_group(&store, profile) {
                Ok(profile) => profile,
                Err(err) => {
                    eprintln!("{}：执行失败：{err:#}", domain);
                    continue;
                }
            };
            match workflow::renew_profile(&runtime_profile, false).await {
                Ok(outcome) => println!("{}：{}", domain, outcome.message),
                Err(err) => eprintln!("{}：执行失败：{err:#}", domain),
            }
        }
    }
}
