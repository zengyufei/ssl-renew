use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use ssl_core::config::default_signer_pipe;
use ssl_core::signer::{
    default_secrets_path, init_config, lock_via_pipe, present_via_pipe, serve, signer_status,
    status_via_pipe, unlock_via_pipe, SignerInitRequest, SignerPresentRequest,
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ssl-signer-agent", version, about = "受限 DNS-01 签发代理")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Init(InitArgs),
    Status(PathArgs),
    Serve(PathArgs),
    Unlock(UnlockArgs),
    Lock(PipeArgs),
    RuntimeStatus(PipeArgs),
    TestPresent(TestPresentArgs),
}

#[derive(Args)]
struct PathArgs {
    #[arg(long)]
    secrets: Option<PathBuf>,
}

#[derive(Args)]
struct InitArgs {
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
struct PipeArgs {
    #[arg(long, default_value_t = default_signer_pipe())]
    pipe_name: String,
}

#[derive(Args)]
struct UnlockArgs {
    #[arg(long, default_value_t = default_signer_pipe())]
    pipe_name: String,
    #[arg(long)]
    passphrase: String,
}

#[derive(Args)]
struct TestPresentArgs {
    #[arg(long, default_value_t = default_signer_pipe())]
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli
        .command
        .unwrap_or(Commands::Serve(PathArgs { secrets: None }))
    {
        Commands::Init(args) => {
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
        Commands::Status(args) => {
            println!(
                "{}",
                signer_status(args.secrets.unwrap_or_else(default_secrets_path))?
            );
        }
        Commands::Serve(args) => {
            serve(args.secrets.unwrap_or_else(default_secrets_path)).await?;
        }
        Commands::Unlock(args) => {
            let response = unlock_via_pipe(&args.pipe_name, args.passphrase).await?;
            println!("{}", response.message);
            if !response.ok {
                std::process::exit(1);
            }
        }
        Commands::Lock(args) => {
            let response = lock_via_pipe(&args.pipe_name).await?;
            println!("{}", response.message);
            if !response.ok {
                std::process::exit(1);
            }
        }
        Commands::RuntimeStatus(args) => {
            let response = status_via_pipe(&args.pipe_name).await?;
            println!("{}", response.message);
            if let Some(status) = response.status {
                println!("{}", serde_json::to_string_pretty(&status)?);
            }
            if !response.ok {
                std::process::exit(1);
            }
        }
        Commands::TestPresent(args) => {
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
                std::process::exit(1);
            }
        }
    }
    Ok(())
}
