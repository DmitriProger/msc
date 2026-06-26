#![allow(dead_code)]
use chrono::{DateTime, Utc};

pub fn next_cron_run(cron_expr: &str, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    use std::str::FromStr;
    let schedule = cron::Schedule::from_str(cron_expr).ok()?;
    schedule.after(&after).next()
}

pub fn sleep_until(target: DateTime<Utc>) {
    let now = Utc::now();
    if target > now {
        let duration = (target - now).to_std().unwrap_or(std::time::Duration::ZERO);
        std::thread::sleep(duration);
    }
}
