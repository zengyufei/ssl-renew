use crate::config::{DnsProviderKind, Profile};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::Engine;
use chrono::Utc;
use hickory_resolver::config::{NameServerConfigGroup, ResolverConfig, ResolverOpts};
use hickory_resolver::TokioAsyncResolver;
use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::Sha1;
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Duration;
use tokio::time::sleep;

const ALI_SAFE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DnsChallengeInfo {
    pub domain: String,
    pub txt_name: String,
    pub rr_name: String,
    pub txt_value: String,
}

#[async_trait]
pub trait DnsProvider: Send + Sync {
    async fn present(&self, challenge: &DnsChallengeInfo) -> Result<()>;
    async fn cleanup(&self, challenge: &DnsChallengeInfo) -> Result<()> {
        println!(
            "保留 TXT 记录，不删除：{} = {}",
            challenge.txt_name, challenge.txt_value
        );
        Ok(())
    }
}

pub struct ManualDnsProvider;

#[async_trait]
impl DnsProvider for ManualDnsProvider {
    async fn present(&self, challenge: &DnsChallengeInfo) -> Result<()> {
        println!("手动 DNS 模式，请添加 TXT：");
        println!("名称：{}", challenge.txt_name);
        println!("主机记录：{}", challenge.rr_name);
        println!("值：{}", challenge.txt_value);
        Ok(())
    }
}

pub struct AliyunDnsProvider {
    client: Client,
    access_key_id: String,
    access_key_secret: String,
    root_domain: String,
    endpoint: String,
}

#[async_trait]
impl DnsProvider for AliyunDnsProvider {
    async fn present(&self, challenge: &DnsChallengeInfo) -> Result<()> {
        let rr = rr_from_txt_name(&challenge.txt_name, &self.root_domain)?;
        if let Some(record) = self.find_record_by_rr(&rr).await? {
            if record.value == challenge.txt_value {
                println!("阿里云 TXT 记录已是当前值：{}", challenge.txt_name);
                return Ok(());
            }
            let mut params = BTreeMap::new();
            params.insert("Action".to_string(), "UpdateDomainRecord".to_string());
            params.insert("RecordId".to_string(), record.record_id);
            params.insert("RR".to_string(), rr);
            params.insert("Type".to_string(), "TXT".to_string());
            params.insert("Value".to_string(), challenge.txt_value.clone());
            params.insert("TTL".to_string(), "600".to_string());
            self.request(params).await?;
            println!("已更新阿里云 TXT 记录：{}", challenge.txt_name);
            return Ok(());
        }
        let mut params = BTreeMap::new();
        params.insert("Action".to_string(), "AddDomainRecord".to_string());
        params.insert("DomainName".to_string(), self.root_domain.clone());
        params.insert("RR".to_string(), rr);
        params.insert("Type".to_string(), "TXT".to_string());
        params.insert("Value".to_string(), challenge.txt_value.clone());
        params.insert("TTL".to_string(), "600".to_string());
        self.request(params).await?;
        println!("已添加阿里云 TXT 记录：{}", challenge.txt_name);
        Ok(())
    }
}

impl AliyunDnsProvider {
    pub fn new(
        access_key_id: String,
        access_key_secret: String,
        root_domain: String,
        endpoint: String,
    ) -> Self {
        Self {
            client: Client::new(),
            access_key_id,
            access_key_secret,
            root_domain,
            endpoint,
        }
    }

    async fn find_record_by_rr(&self, rr: &str) -> Result<Option<AliRecord>> {
        let mut params = BTreeMap::new();
        params.insert("Action".to_string(), "DescribeDomainRecords".to_string());
        params.insert("DomainName".to_string(), self.root_domain.clone());
        params.insert("RRKeyWord".to_string(), rr.to_string());
        params.insert("Type".to_string(), "TXT".to_string());
        let body = self.request(params).await?;
        let records = body
            .get("DomainRecords")
            .and_then(|v| v.get("Record"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for record in records {
            if record.get("RR").and_then(Value::as_str) == Some(rr) {
                return Ok(Some(AliRecord {
                    record_id: record
                        .get("RecordId")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    value: record
                        .get("Value")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                }));
            }
        }
        Ok(None)
    }

    async fn request(&self, params: BTreeMap<String, String>) -> Result<Value> {
        let mut signed = BTreeMap::new();
        signed.insert("Format".to_string(), "JSON".to_string());
        signed.insert("Version".to_string(), "2015-01-09".to_string());
        signed.insert("AccessKeyId".to_string(), self.access_key_id.clone());
        signed.insert("SignatureMethod".to_string(), "HMAC-SHA1".to_string());
        signed.insert(
            "Timestamp".to_string(),
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        );
        signed.insert("SignatureVersion".to_string(), "1.0".to_string());
        signed.insert(
            "SignatureNonce".to_string(),
            uuid::Uuid::new_v4().to_string(),
        );
        signed.extend(params);
        let signature = self.signature(&signed)?;
        signed.insert("Signature".to_string(), signature);
        let response = self
            .client
            .get(&self.endpoint)
            .query(&signed)
            .send()
            .await?;
        let status = response.status();
        let body: Value = response.json().await?;
        if !status.is_success() || body.get("Code").is_some() {
            return Err(anyhow!("阿里云 API 错误：{body}"));
        }
        Ok(body)
    }

    fn signature(&self, params: &BTreeMap<String, String>) -> Result<String> {
        let canonical = params
            .iter()
            .map(|(k, v)| format!("{}={}", percent(k), percent(v)))
            .collect::<Vec<_>>()
            .join("&");
        let string_to_sign = format!("GET&%2F&{}", percent(&canonical));
        let mut mac =
            Hmac::<Sha1>::new_from_slice(format!("{}&", self.access_key_secret).as_bytes())?;
        mac.update(string_to_sign.as_bytes());
        Ok(base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes()))
    }
}

#[derive(Clone, Debug)]
struct AliRecord {
    record_id: String,
    value: String,
}

pub struct CloudflareDnsProvider {
    client: Client,
    token: String,
    root_domain: String,
    endpoint: String,
}

#[async_trait]
impl DnsProvider for CloudflareDnsProvider {
    async fn present(&self, challenge: &DnsChallengeInfo) -> Result<()> {
        let zone_id = self.zone_id().await?;
        if let Some(record) = self.find_record(&zone_id, &challenge.txt_name).await? {
            if record.content == challenge.txt_value {
                println!("Cloudflare TXT 记录已是当前值：{}", challenge.txt_name);
                return Ok(());
            }
            let body = serde_json::json!({"type":"TXT","name":challenge.txt_name,"content":challenge.txt_value,"ttl":120});
            self.request(
                reqwest::Method::PUT,
                &format!("/zones/{zone_id}/dns_records/{}", record.id),
                Some(body),
            )
            .await?;
            println!("已更新 Cloudflare TXT 记录：{}", challenge.txt_name);
            return Ok(());
        }
        let body = serde_json::json!({"type":"TXT","name":challenge.txt_name,"content":challenge.txt_value,"ttl":120});
        self.request(
            reqwest::Method::POST,
            &format!("/zones/{zone_id}/dns_records"),
            Some(body),
        )
        .await?;
        println!("已添加 Cloudflare TXT 记录：{}", challenge.txt_name);
        Ok(())
    }
}

impl CloudflareDnsProvider {
    pub fn new(token: String, root_domain: String, endpoint: String) -> Self {
        Self {
            client: Client::new(),
            token,
            root_domain,
            endpoint,
        }
    }

    async fn zone_id(&self) -> Result<String> {
        let value = self
            .request(
                reqwest::Method::GET,
                &format!("/zones?name={}&status=active", self.root_domain),
                None,
            )
            .await?;
        value
            .get("result")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Cloudflare 未找到 Zone：{}", self.root_domain))
    }

    async fn find_record(&self, zone_id: &str, txt_name: &str) -> Result<Option<CfRecord>> {
        let value = self
            .request(
                reqwest::Method::GET,
                &format!("/zones/{zone_id}/dns_records?type=TXT&name={txt_name}"),
                None,
            )
            .await?;
        let Some(first) = value
            .get("result")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
        else {
            return Ok(None);
        };
        Ok(Some(CfRecord {
            id: first
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            content: first
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }))
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let mut request = self
            .client
            .request(
                method,
                format!("{}{}", self.endpoint.trim_end_matches('/'), path),
            )
            .bearer_auth(&self.token)
            .header("Content-Type", "application/json");
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await?;
        let status = response.status();
        let body: Value = response.json().await?;
        if !status.is_success() || body.get("success").and_then(Value::as_bool) == Some(false) {
            return Err(anyhow!("Cloudflare API 错误：{body}"));
        }
        Ok(body)
    }
}

struct CfRecord {
    id: String,
    content: String,
}

pub fn build_provider(profile: &Profile) -> Result<Box<dyn DnsProvider>> {
    let kind = DnsProviderKind::from_value(&profile.dns.provider);
    let client = Client::new();
    match kind {
        DnsProviderKind::Manual => Ok(Box::new(ManualDnsProvider)),
        DnsProviderKind::Aliyun => {
            let key_id =
                read_env_value(&profile.dns.aliyun.access_key_id_env).ok_or_else(|| {
                    anyhow!(
                        "未检测到阿里云环境变量：{}",
                        profile.dns.aliyun.access_key_id_env
                    )
                })?;
            let key_secret =
                read_env_value(&profile.dns.aliyun.access_key_secret_env).ok_or_else(|| {
                    anyhow!(
                        "未检测到阿里云环境变量：{}",
                        profile.dns.aliyun.access_key_secret_env
                    )
                })?;
            Ok(Box::new(AliyunDnsProvider {
                client,
                access_key_id: key_id,
                access_key_secret: key_secret,
                root_domain: profile
                    .dns
                    .aliyun
                    .root_domain
                    .clone()
                    .unwrap_or_else(|| derive_root_domain(&profile.domain)),
                endpoint: profile.dns.aliyun.endpoint.clone(),
            }))
        }
        DnsProviderKind::Cloudflare => {
            let token = read_env_value(&profile.dns.cloudflare.api_token_env).ok_or_else(|| {
                anyhow!(
                    "未检测到 Cloudflare 环境变量：{}",
                    profile.dns.cloudflare.api_token_env
                )
            })?;
            Ok(Box::new(CloudflareDnsProvider {
                client,
                token,
                root_domain: profile
                    .dns
                    .cloudflare
                    .root_domain
                    .clone()
                    .unwrap_or_else(|| derive_root_domain(&profile.domain)),
                endpoint: profile.dns.cloudflare.endpoint.clone(),
            }))
        }
        DnsProviderKind::Signer => Ok(Box::new(crate::signer::SignerDnsProvider::new(
            profile.dns.signer.pipe_name.clone(),
        ))),
    }
}

pub async fn wait_for_records(
    challenges: &[DnsChallengeInfo],
    resolvers: &[String],
    timeout_seconds: u64,
    interval_seconds: u64,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_seconds);
    loop {
        let mut pending = Vec::new();
        for challenge in challenges {
            if !txt_is_visible(&challenge.txt_name, &challenge.txt_value, resolvers).await {
                pending.push(challenge.txt_name.clone());
            }
        }
        if pending.is_empty() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!("DNS TXT 记录超时未生效：{}", pending.join(", ")));
        }
        println!("等待 DNS TXT 生效：{}", pending.join(", "));
        sleep(Duration::from_secs(interval_seconds)).await;
    }
}

pub async fn txt_is_visible(txt_name: &str, txt_value: &str, resolvers: &[String]) -> bool {
    let resolver = resolver(resolvers);
    let Ok(response) = resolver.txt_lookup(txt_name).await else {
        return false;
    };
    response.iter().any(|txt| {
        txt.txt_data()
            .iter()
            .map(|part| String::from_utf8_lossy(part).to_string())
            .collect::<String>()
            == txt_value
    })
}

fn resolver(resolvers: &[String]) -> TokioAsyncResolver {
    let ips = resolvers
        .iter()
        .filter_map(|item| item.parse::<IpAddr>().ok())
        .collect::<Vec<_>>();
    if ips.is_empty() {
        TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default())
    } else {
        TokioAsyncResolver::tokio(
            ResolverConfig::from_parts(
                None,
                vec![],
                NameServerConfigGroup::from_ips_clear(&ips, 53, true),
            ),
            ResolverOpts::default(),
        )
    }
}

pub fn derive_root_domain(domain: &str) -> String {
    let clean = domain
        .trim()
        .trim_end_matches('.')
        .strip_prefix("*.")
        .unwrap_or(domain.trim());
    let parts = clean.split('.').collect::<Vec<_>>();
    if parts.len() < 2 {
        return clean.to_string();
    }
    parts[parts.len() - 2..].join(".")
}

pub fn rr_from_txt_name(txt_name: &str, root_domain: &str) -> Result<String> {
    let name = txt_name.trim_end_matches('.');
    let suffix = format!(".{}", root_domain.trim_end_matches('.'));
    if name == root_domain {
        return Ok("@".to_string());
    }
    if !name.ends_with(&suffix) {
        return Err(anyhow!("TXT 名称 {txt_name} 不属于根域名 {root_domain}"));
    }
    Ok(name.trim_end_matches(&suffix).to_string())
}

pub fn dns_rr_name(txt_name: &str, domain: &str) -> String {
    let root = derive_root_domain(domain);
    rr_from_txt_name(txt_name, &root).unwrap_or_else(|_| txt_name.trim_end_matches('.').to_string())
}

fn percent(value: &str) -> String {
    utf8_percent_encode(value, ALI_SAFE).to_string()
}

pub fn read_env_value(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rr_name_for_wildcard_root() {
        assert_eq!(
            dns_rr_name("_acme-challenge.h5por.com", "*.h5por.com"),
            "_acme-challenge"
        );
    }

    #[test]
    fn rr_name_for_wildcard_subdomain() {
        assert_eq!(
            dns_rr_name("_acme-challenge.api.h5por.com", "*.api.h5por.com"),
            "_acme-challenge.api"
        );
    }
}
