use crate::config::{state_dir_for, work_dir_for, Profile};
use crate::dns::{dns_rr_name, DnsChallengeInfo};
use anyhow::{anyhow, Context, Result};
use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier, LetsEncrypt,
    NewAccount, NewOrder, Order, OrderStatus, RetryPolicy,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderSession {
    pub order_url: String,
    pub challenges: Vec<DnsChallengeInfo>,
}

pub struct RuntimeOrder {
    pub account: Account,
    pub order: Order,
    pub session: OrderSession,
}

pub async fn load_or_create_account(profile: &Profile) -> Result<Account> {
    let state_dir = state_dir_for(profile);
    fs::create_dir_all(&state_dir)?;
    let credentials_file = state_dir.join("account.json");
    if credentials_file.exists() {
        let text = fs::read_to_string(&credentials_file)?;
        let credentials: AccountCredentials = serde_json::from_str(&text)?;
        return Ok(Account::builder()?.from_credentials(credentials).await?);
    }
    let contact = format!("mailto:{}", profile.email);
    let (account, credentials) = Account::builder()?
        .create(
            &NewAccount {
                contact: &[contact.as_str()],
                terms_of_service_agreed: true,
                only_return_existing: false,
            },
            LetsEncrypt::Production.url().to_owned(),
            None,
        )
        .await?;
    fs::write(
        credentials_file,
        serde_json::to_string_pretty(&credentials)?,
    )?;
    Ok(account)
}

pub async fn new_order(profile: &Profile) -> Result<RuntimeOrder> {
    let account = load_or_create_account(profile).await?;
    let identifiers = vec![Identifier::Dns(profile.domain.clone())];
    let mut order = account.new_order(&NewOrder::new(&identifiers)).await?;
    let challenges = collect_dns_challenges(&mut order).await?;
    let session = OrderSession {
        order_url: order.url().to_string(),
        challenges,
    };
    save_order_session(profile, &session)?;
    Ok(RuntimeOrder {
        account,
        order,
        session,
    })
}

pub async fn resume_order(profile: &Profile) -> Result<RuntimeOrder> {
    let account = load_or_create_account(profile).await?;
    let session = load_order_session(profile)?;
    let order = account.order(session.order_url.clone()).await?;
    Ok(RuntimeOrder {
        account,
        order,
        session,
    })
}

pub async fn collect_dns_challenges(order: &mut Order) -> Result<Vec<DnsChallengeInfo>> {
    let mut challenges = Vec::new();
    let mut authorizations = order.authorizations();
    while let Some(result) = authorizations.next().await {
        let mut authz = result?;
        if authz.status == AuthorizationStatus::Valid {
            continue;
        }
        if authz.status != AuthorizationStatus::Pending {
            return Err(anyhow!("授权状态异常：{:?}", authz.status));
        }
        let challenge = authz
            .challenge(ChallengeType::Dns01)
            .ok_or_else(|| anyhow!("没有找到 dns-01 challenge"))?;
        let raw_domain = challenge.identifier().to_string();
        let base_domain = clean_dns_identifier(&raw_domain);
        let txt_name = format!("_acme-challenge.{base_domain}");
        let txt_value = challenge.key_authorization().dns_value();
        let rr_name = dns_rr_name(&txt_name, &base_domain);
        challenges.push(DnsChallengeInfo {
            domain: base_domain,
            txt_name,
            rr_name,
            txt_value,
        });
    }
    Ok(challenges)
}

pub async fn trigger_dns_challenges(order: &mut Order) -> Result<()> {
    let mut authorizations = order.authorizations();
    while let Some(result) = authorizations.next().await {
        let mut authz = result?;
        if authz.status == AuthorizationStatus::Valid {
            continue;
        }
        let mut challenge = authz
            .challenge(ChallengeType::Dns01)
            .ok_or_else(|| anyhow!("没有找到 dns-01 challenge"))?;
        challenge.set_ready().await?;
    }
    Ok(())
}

pub async fn finalize_and_download(order: &mut Order) -> Result<(String, String)> {
    let status = order.poll_ready(&RetryPolicy::default()).await?;
    if status != OrderStatus::Ready {
        return Err(anyhow!("订单状态不是 Ready：{status:?}"));
    }
    let private_key_pem = order.finalize().await?;
    let cert_chain_pem = order.poll_certificate(&RetryPolicy::default()).await?;
    Ok((private_key_pem, cert_chain_pem))
}

pub fn save_order_session(profile: &Profile, session: &OrderSession) -> Result<()> {
    let path = order_session_path(profile);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(session)?)?;
    Ok(())
}

pub fn load_order_session(profile: &Profile) -> Result<OrderSession> {
    let path = order_session_path(profile);
    let text = fs::read_to_string(&path)
        .with_context(|| format!("读取订单会话失败：{}", path.display()))?;
    let mut session: OrderSession = serde_json::from_str(&text)?;
    normalize_session_challenges(&mut session);
    Ok(session)
}

pub fn order_session_path(profile: &Profile) -> PathBuf {
    work_dir_for(profile).join("current-order.json")
}

fn normalize_session_challenges(session: &mut OrderSession) {
    for challenge in &mut session.challenges {
        let clean_domain = clean_dns_identifier(&challenge.domain);
        let txt_name = if challenge.txt_name.contains(".*.") {
            challenge.txt_name.replace(".*.", ".")
        } else {
            format!("_acme-challenge.{clean_domain}")
        };
        challenge.domain = clean_domain.clone();
        challenge.txt_name = txt_name.trim_end_matches('.').to_string();
        challenge.rr_name = dns_rr_name(&challenge.txt_name, &clean_domain);
    }
}

fn clean_dns_identifier(value: &str) -> String {
    value
        .trim()
        .trim_end_matches('.')
        .strip_prefix("*.")
        .unwrap_or(value.trim().trim_end_matches('.'))
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_identifier_uses_root_txt_name() {
        assert_eq!(clean_dns_identifier("*.h5por.com"), "h5por.com");
    }

    #[test]
    fn normalizes_old_wildcard_order_session() {
        let mut session = OrderSession {
            order_url: "https://example.com/order".to_string(),
            challenges: vec![DnsChallengeInfo {
                domain: "*.h5por.com".to_string(),
                txt_name: "_acme-challenge.*.h5por.com".to_string(),
                rr_name: "_acme-challenge.*".to_string(),
                txt_value: "value".to_string(),
            }],
        };
        normalize_session_challenges(&mut session);
        assert_eq!(session.challenges[0].domain, "h5por.com");
        assert_eq!(session.challenges[0].txt_name, "_acme-challenge.h5por.com");
        assert_eq!(session.challenges[0].rr_name, "_acme-challenge");
    }
}
