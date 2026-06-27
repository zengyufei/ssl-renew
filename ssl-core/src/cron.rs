use anyhow::{anyhow, Result};
use chrono::{DateTime, Datelike, Duration, Local, Timelike};
use std::collections::BTreeSet;

fn parse_field(field: &str, min: u32, max: u32, weekday: bool) -> Result<BTreeSet<u32>> {
    let mut values = BTreeSet::new();
    for part in field.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(anyhow!("Cron 表达式包含空字段"));
        }
        let (base, step) = if let Some((base, step)) = part.split_once('/') {
            let step: u32 = step.parse()?;
            if step == 0 {
                return Err(anyhow!("Cron 步进值必须大于 0"));
            }
            (base, step)
        } else {
            (part, 1)
        };
        let (start, end) = if base == "*" {
            (min, max)
        } else if let Some((a, b)) = base.split_once('-') {
            (a.parse::<u32>()?, b.parse::<u32>()?)
        } else {
            let value = base.parse::<u32>()?;
            (value, value)
        };
        if start > end || start < min || start > max || end < min || end > max {
            return Err(anyhow!("Cron 字段超出范围：{part}"));
        }
        for value in (start..=end).step_by(step as usize) {
            values.insert(value);
        }
    }
    if weekday && values.remove(&7) {
        values.insert(0);
    }
    Ok(values)
}

pub fn cron_next_run(expression: &str, now: DateTime<Local>) -> Result<DateTime<Local>> {
    let parts: Vec<_> = expression.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(anyhow!(
            "Cron 表达式必须是 5 段：分钟 小时 日 月 周，例如 0 10 * * *"
        ));
    }
    let minutes = parse_field(parts[0], 0, 59, false)?;
    let hours = parse_field(parts[1], 0, 23, false)?;
    let days = parse_field(parts[2], 1, 31, false)?;
    let months = parse_field(parts[3], 1, 12, false)?;
    let weekdays = parse_field(parts[4], 0, 7, true)?;
    let day_star = parts[2] == "*";
    let weekday_star = parts[4] == "*";
    let mut current = now
        .with_second(0)
        .and_then(|v| v.with_nanosecond(0))
        .ok_or_else(|| anyhow!("无法计算当前时间"))?
        + Duration::minutes(1);
    let deadline = current + Duration::days(366 * 5);
    while current <= deadline {
        if !minutes.contains(&current.minute())
            || !hours.contains(&current.hour())
            || !months.contains(&current.month())
        {
            current += Duration::minutes(1);
            continue;
        }
        let day_match = days.contains(&current.day());
        let weekday_value = current.weekday().num_days_from_sunday();
        let weekday_match = weekdays.contains(&weekday_value);
        let calendar_match = match (day_star, weekday_star) {
            (true, true) => true,
            (true, false) => weekday_match,
            (false, true) => day_match,
            (false, false) => day_match || weekday_match,
        };
        if calendar_match {
            return Ok(current);
        }
        current += Duration::minutes(1);
    }
    Err(anyhow!("未来 5 年内找不到匹配的 Cron 执行时间"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn daily_ten() {
        let now = Local.with_ymd_and_hms(2026, 6, 24, 9, 59, 0).unwrap();
        let next = cron_next_run("0 10 * * *", now).unwrap();
        assert_eq!(next.hour(), 10);
        assert_eq!(next.minute(), 0);
    }

    #[test]
    fn every_thirty_minutes() {
        let now = Local.with_ymd_and_hms(2026, 6, 24, 10, 1, 0).unwrap();
        let next = cron_next_run("*/30 * * * *", now).unwrap();
        assert_eq!(next.minute(), 30);
    }
}
