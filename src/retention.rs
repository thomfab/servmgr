use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Days, Local, NaiveTime, TimeZone, Utc};
use tracing::{error, info};

use crate::db;
use crate::engine::AppState;

pub const LAST_PURGE_KEY: &str = "last_history_purge";

/// Local hour at which the daily purge is scheduled.
const PURGE_HOUR: u32 = 1;
/// How often we wake up to check whether a purge is due. Polling (rather than
/// sleeping until 01:00) keeps us correct across clock changes and suspend.
const POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// The most recent scheduled purge time at or before `now` (today's 01:00 if
/// already past, otherwise yesterday's). A purge is due whenever the last
/// recorded run is older than this, which also catches up runs missed while
/// the service was down.
fn last_scheduled_run<Tz: TimeZone>(now: &DateTime<Tz>) -> DateTime<Utc> {
    let tz = now.timezone();
    let at_hour = |date: chrono::NaiveDate| {
        let naive = date.and_time(NaiveTime::from_hms_opt(PURGE_HOUR, 0, 0).unwrap());
        // `earliest` handles DST overlaps; on a DST gap fall back to an hour later.
        tz.from_local_datetime(&naive)
            .earliest()
            .or_else(|| tz.from_local_datetime(&(naive + chrono::Duration::hours(1))).earliest())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|| naive.and_utc())
    };
    let today = now.date_naive();
    let today_run = at_hour(today);
    if now.with_timezone(&Utc) >= today_run {
        today_run
    } else {
        at_hour(today.checked_sub_days(Days::new(1)).unwrap_or(today))
    }
}

pub fn start_purge_task(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            if let Err(e) = purge_if_due(&state).await {
                error!("History purge failed: {e}");
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}

async fn purge_if_due(state: &AppState) -> Result<(), sqlx::Error> {
    let due = last_scheduled_run(&Local::now());
    let last = db::get_meta(&state.pool, LAST_PURGE_KEY)
        .await?
        .and_then(|s| s.parse::<DateTime<Utc>>().ok());
    if last.is_some_and(|l| l >= due) {
        return Ok(());
    }

    let days = state.config.read().await.history_retention_days;
    if days > 0 {
        let cutoff = Utc::now() - chrono::Duration::days(days as i64);
        let deleted = db::purge_history_before(&state.pool, cutoff).await?;
        info!("History purge: removed {deleted} rows older than {days} days");
    }
    db::set_meta(&state.pool, LAST_PURGE_KEY, &Utc::now().to_rfc3339()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn at(tz: &FixedOffset, s: &str) -> DateTime<FixedOffset> {
        tz.from_local_datetime(&s.parse().unwrap()).unwrap()
    }

    #[test]
    fn before_purge_hour_uses_yesterday() {
        let tz = FixedOffset::east_opt(3600).unwrap();
        let now = at(&tz, "2026-11-26T00:30:00");
        assert_eq!(last_scheduled_run(&now), at(&tz, "2026-11-25T01:00:00"));
    }

    #[test]
    fn after_purge_hour_uses_today() {
        let tz = FixedOffset::east_opt(3600).unwrap();
        let now = at(&tz, "2026-11-26T14:00:00");
        assert_eq!(last_scheduled_run(&now), at(&tz, "2026-11-26T01:00:00"));
    }

    #[test]
    fn exactly_at_purge_hour_uses_today() {
        let tz = FixedOffset::west_opt(5 * 3600).unwrap();
        let now = at(&tz, "2026-11-26T01:00:00");
        assert_eq!(last_scheduled_run(&now), now.with_timezone(&Utc));
    }
}
