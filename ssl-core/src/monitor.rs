use crate::config::{MonitorConfig, Profile};
use crate::cron::cron_next_run;
use anyhow::{anyhow, Result};
use chrono::{Duration, Local, NaiveTime, TimeZone};

pub fn next_monitor_run(config: &MonitorConfig) -> Result<chrono::DateTime<Local>> {
    let now = Local::now();
    match config.mode.as_str() {
        "cron" => cron_next_run(&config.cron_expression, now),
        "interval" => Ok(now + Duration::minutes(config.interval_minutes.max(1) as i64)),
        _ => {
            let time = NaiveTime::parse_from_str(&config.daily_time, "%H:%M")?;
            let today = now.date_naive().and_time(time);
            let mut candidate = Local
                .from_local_datetime(&today)
                .single()
                .ok_or_else(|| anyhow!("无法计算监控时间"))?;
            if candidate <= now {
                candidate += Duration::days(1);
            }
            Ok(candidate)
        }
    }
}

pub fn selected_profiles<'a>(
    config: &MonitorConfig,
    profiles: &'a std::collections::BTreeMap<String, Profile>,
) -> Vec<(&'a str, &'a Profile)> {
    config
        .profiles
        .iter()
        .filter_map(|domain| {
            profiles
                .get_key_value(domain)
                .map(|(key, profile)| (key.as_str(), profile))
        })
        .collect()
}
