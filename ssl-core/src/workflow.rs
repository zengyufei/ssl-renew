use crate::acme::{self, RuntimeOrder};
use crate::cert;
use crate::config::{DnsProviderKind, Profile};
use crate::dns::{self, DnsChallengeInfo};
use crate::logging::reset_log_if_too_large;
use crate::nginx;
use anyhow::{Context, Result};
use chrono::Local;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

pub use crate::cert::CertificateStatus;

#[derive(Clone, Debug, Serialize)]
pub struct RenewOutcome {
    pub renewed: bool,
    pub message: String,
    pub challenges: Vec<DnsChallengeInfo>,
}

pub async fn check_certificate(profile: &Profile, force: bool) -> Result<CertificateStatus> {
    crate::install_default_crypto_provider();
    maybe_reset_log(profile)?;
    cert::cert_status(
        &profile.paths.cert_file,
        profile.renew.days_before_expiry,
        force,
    )
}

pub async fn create_order_prepare_dns(profile: &Profile) -> Result<RuntimeOrder> {
    crate::install_default_crypto_provider();
    maybe_reset_log(profile)?;
    let runtime = acme::new_order(profile).await?;
    present_dns(profile, &runtime.session.challenges).await?;
    Ok(runtime)
}

pub async fn present_dns(profile: &Profile, challenges: &[DnsChallengeInfo]) -> Result<()> {
    crate::install_default_crypto_provider();
    let provider = dns::build_provider(profile)?;
    for challenge in challenges {
        provider.present(challenge).await?;
    }
    Ok(())
}

pub async fn dns_records_visible(
    profile: &Profile,
    challenges: &[DnsChallengeInfo],
) -> Result<bool> {
    crate::install_default_crypto_provider();
    for challenge in challenges {
        if !dns::txt_is_visible(
            &challenge.txt_name,
            &challenge.txt_value,
            &profile.dns.resolvers,
        )
        .await
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub async fn wait_for_dns(profile: &Profile, challenges: &[DnsChallengeInfo]) -> Result<()> {
    crate::install_default_crypto_provider();
    dns::wait_for_records(
        challenges,
        &profile.dns.resolvers,
        profile.dns.propagation_timeout_seconds,
        profile.dns.propagation_interval_seconds,
    )
    .await
}

pub async fn issue_certificate(profile: &Profile) -> Result<()> {
    crate::install_default_crypto_provider();
    maybe_reset_log(profile)?;
    let mut runtime = acme::resume_order(profile).await?;
    wait_for_dns(profile, &runtime.session.challenges).await?;
    acme::trigger_dns_challenges(&mut runtime.order).await?;
    let (private_key_pem, cert_chain_pem) = acme::finalize_and_download(&mut runtime.order).await?;
    save_certificate_files(
        profile,
        cert_chain_pem.as_bytes(),
        private_key_pem.as_bytes(),
    )?;
    Ok(())
}

pub async fn renew_profile(profile: &Profile, force: bool) -> Result<RenewOutcome> {
    crate::install_default_crypto_provider();
    maybe_reset_log(profile)?;
    let status = check_certificate(profile, force).await?;
    if !status.should_renew {
        return Ok(RenewOutcome {
            renewed: false,
            message: "当前证书未达到续期阈值，本轮不申请".to_string(),
            challenges: vec![],
        });
    }
    if DnsProviderKind::from_value(&profile.dns.provider) == DnsProviderKind::Manual {
        return Err(anyhow::anyhow!(
            "当前配置是手动DNS，无法无人值守执行完整 renew"
        ));
    }
    let mut runtime = create_order_prepare_dns(profile).await?;
    wait_for_dns(profile, &runtime.session.challenges).await?;
    acme::trigger_dns_challenges(&mut runtime.order).await?;
    let (private_key_pem, cert_chain_pem) = acme::finalize_and_download(&mut runtime.order).await?;
    save_certificate_files(
        profile,
        cert_chain_pem.as_bytes(),
        private_key_pem.as_bytes(),
    )?;
    restart_nginx_for_profile(profile).await?;
    Ok(RenewOutcome {
        renewed: true,
        message: "证书续期、保存和 Nginx 重启完成".to_string(),
        challenges: runtime.session.challenges,
    })
}

pub async fn restart_nginx_for_profile(profile: &Profile) -> Result<()> {
    nginx::restart_nginx(&profile.nginx).await
}

pub fn load_saved_challenges(profile: &Profile) -> Result<Vec<DnsChallengeInfo>> {
    Ok(acme::load_order_session(profile)?.challenges)
}

fn save_certificate_files(profile: &Profile, cert_pem: &[u8], key_pem: &[u8]) -> Result<()> {
    let cert_file = PathBuf::from(&profile.paths.cert_file);
    let key_file = PathBuf::from(&profile.paths.key_file);
    let backup_dir = PathBuf::from(&profile.paths.backup_dir);
    fs::create_dir_all(cert_file.parent().unwrap_or_else(|| Path::new(".")))?;
    fs::create_dir_all(key_file.parent().unwrap_or_else(|| Path::new(".")))?;
    fs::create_dir_all(&backup_dir)?;
    let stamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
    backup_existing(&cert_file, &backup_dir, &stamp)?;
    backup_existing(&key_file, &backup_dir, &stamp)?;
    atomic_write(&cert_file, cert_pem)?;
    atomic_write(&key_file, key_pem)?;
    Ok(())
}

fn backup_existing(path: &Path, backup_dir: &Path, stamp: &str) -> Result<()> {
    if path.exists() {
        let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("cert");
        fs::copy(path, backup_dir.join(format!("{name}.{stamp}.bak")))?;
    }
    Ok(())
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let temp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|v| v.to_str()).unwrap_or("new")
    ));
    fs::write(&temp, data).with_context(|| format!("写入临时文件失败：{}", temp.display()))?;
    fs::rename(&temp, path).or_else(|_| {
        fs::copy(&temp, path)?;
        fs::remove_file(&temp)?;
        Ok::<_, std::io::Error>(())
    })?;
    Ok(())
}

fn maybe_reset_log(profile: &Profile) -> Result<()> {
    reset_log_if_too_large(&profile.paths.log_file, profile.paths.max_log_size_mb)?;
    Ok(())
}
