pub mod acme;
pub mod cert;
pub mod config;
pub mod cron;
pub mod dns;
pub mod logging;
pub mod monitor;
pub mod nginx;
pub mod signer;
pub mod workflow;

use std::sync::Once;

static CRYPTO_PROVIDER_INIT: Once = Once::new();

pub fn install_default_crypto_provider() {
    CRYPTO_PROVIDER_INIT.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

pub use acme::RuntimeOrder;
pub use config::{
    default_monitor_config, default_profile, default_vendor_configs, load_store,
    safe_domain_filename, save_store, DnsProviderKind, Profile, Store,
};
pub use cron::cron_next_run;
pub use dns::DnsChallengeInfo;
pub use workflow::{
    check_certificate, create_order_prepare_dns, dns_records_visible, issue_certificate,
    renew_profile, restart_nginx_for_profile, CertificateStatus,
};
