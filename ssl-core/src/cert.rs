use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use serde::Serialize;
use std::path::{Path, PathBuf};
use x509_parser::pem::parse_x509_pem;

#[derive(Clone, Debug, Serialize)]
pub struct CertificateStatus {
    pub cert_file: PathBuf,
    pub exists: bool,
    pub expires_at: Option<String>,
    pub days_remaining: Option<i64>,
    pub days_before_expiry: i64,
    pub force: bool,
    pub should_renew: bool,
    pub message: String,
}

pub fn cert_status(
    cert_file: impl AsRef<Path>,
    days_before_expiry: i64,
    force: bool,
) -> Result<CertificateStatus> {
    let cert_file = cert_file.as_ref().to_path_buf();
    if !cert_file.exists() {
        let message = if force {
            "当前证书文件不存在，需要申请；同时已勾选强制申请".to_string()
        } else {
            "当前证书文件不存在，需要申请".to_string()
        };
        return Ok(CertificateStatus {
            cert_file,
            exists: false,
            expires_at: None,
            days_remaining: None,
            days_before_expiry,
            force,
            should_renew: true,
            message,
        });
    }
    let pem = std::fs::read(&cert_file)
        .with_context(|| format!("读取证书失败：{}", cert_file.display()))?;
    let (_, pem) =
        parse_x509_pem(&pem).map_err(|err| anyhow::anyhow!("解析 PEM 证书失败：{err}"))?;
    let cert = pem
        .parse_x509()
        .map_err(|err| anyhow::anyhow!("解析 X509 证书失败：{err}"))?;
    let not_after = cert.validity().not_after;
    let expires_at_dt = Utc
        .timestamp_opt(not_after.timestamp(), 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("证书到期时间无效"))?;
    let days_remaining = (expires_at_dt - Utc::now()).num_days();
    let should_renew = force || days_remaining <= days_before_expiry;
    let expires_at = expires_at_dt.to_rfc3339();
    let mut message = format!("当前证书到期时间：{expires_at}，剩余 {days_remaining} 天");
    if force {
        message.push_str("；已勾选强制申请，所以将继续申请");
    } else if should_renew {
        message.push_str(&format!(
            "；小于等于提前续期阈值 {days_before_expiry} 天，需要续期"
        ));
    } else {
        message.push_str(&format!(
            "；大于提前续期阈值 {days_before_expiry} 天，暂不需要续期"
        ));
    }
    Ok(CertificateStatus {
        cert_file,
        exists: true,
        expires_at: Some(expires_at),
        days_remaining: Some(days_remaining),
        days_before_expiry,
        force,
        should_renew,
        message,
    })
}
