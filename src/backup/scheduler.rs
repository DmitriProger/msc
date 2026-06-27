use chrono::{DateTime, Utc};

/// The `cron` crate expects a 6-field expression (seconds first). Accept the
/// standard 5-field crontab syntax (`min hour dom month dow`) by prepending a
/// `0` seconds field, so `0 4 * * *` means "daily at 04:00:00".
fn normalize_cron(expr: &str) -> String {
    if expr.split_whitespace().count() == 5 {
        format!("0 {}", expr.trim())
    } else {
        expr.trim().to_string()
    }
}

pub fn next_cron_run(cron_expr: &str, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    use std::str::FromStr;
    let schedule = cron::Schedule::from_str(&normalize_cron(cron_expr)).ok()?;
    schedule.after(&after).next()
}

#[allow(dead_code)]
pub fn sleep_until(target: DateTime<Utc>) {
    let now = Utc::now();
    if target > now {
        let duration = (target - now).to_std().unwrap_or(std::time::Duration::ZERO);
        std::thread::sleep(duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn five_field_cron_parses_and_advances() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 3, 0, 0).unwrap();
        // daily at 04:00 -> next run same day 04:00:00
        let next = next_cron_run("0 4 * * *", now).unwrap();
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 1, 1, 4, 0, 0).unwrap());
    }

    #[test]
    fn every_minute_five_field() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 3, 0, 30).unwrap();
        let next = next_cron_run("* * * * *", now).unwrap();
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 1, 1, 3, 1, 0).unwrap());
    }

    #[test]
    fn invalid_cron_returns_none() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 3, 0, 0).unwrap();
        assert!(next_cron_run("not a cron", now).is_none());
    }
}
