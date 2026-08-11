use crate::core::{ConcurrencyPolicy, MisfirePolicy, Trigger};
use anyhow::Result;
use chrono::{DateTime, Duration, Local, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use std::str::FromStr;

pub fn next_run(trigger: &Trigger, after: DateTime<Utc>) -> Result<Option<DateTime<Utc>>> {
    match trigger {
        Trigger::Manual => Ok(None),
        Trigger::Interval { seconds } if *seconds == 0 => {
            anyhow::bail!("interval must be greater than zero")
        }
        Trigger::Interval { seconds } if *seconds > i64::MAX as u64 => {
            anyhow::bail!("interval exceeds the supported duration range")
        }
        Trigger::Interval { seconds } => {
            let duration = Duration::try_seconds(*seconds as i64)
                .ok_or_else(|| anyhow::anyhow!("interval exceeds the supported duration range"))?;
            after
                .checked_add_signed(duration)
                .map(Some)
                .ok_or_else(|| anyhow::anyhow!("interval exceeds the supported date range"))
        }
        Trigger::Cron {
            expression,
            timezone,
        } => {
            let normalized = normalize_cron_expression(expression)?;
            let schedule = Schedule::from_str(&normalized).map_err(|error| {
                anyhow::anyhow!("invalid cron expression {expression:?}: {error}")
            })?;
            match timezone.trim() {
                "" => anyhow::bail!("cron timezone must not be empty"),
                value if value.eq_ignore_ascii_case("utc") => Ok(schedule
                    .after(&after)
                    .next()
                    .map(|value| value.with_timezone(&Utc))),
                value if value.eq_ignore_ascii_case("local") => Ok(schedule
                    .after(&after.with_timezone(&Local))
                    .next()
                    .map(|value| value.with_timezone(&Utc))),
                value => {
                    let timezone = value.parse::<Tz>().map_err(|error| {
                        anyhow::anyhow!("invalid IANA cron timezone {value:?}: {error}")
                    })?;
                    Ok(schedule
                        .after(&after.with_timezone(&timezone))
                        .next()
                        .map(|value| value.with_timezone(&Utc)))
                }
            }
        }
    }
}

fn normalize_cron_expression(expression: &str) -> Result<String> {
    let fields = expression.split_whitespace().collect::<Vec<_>>();
    match fields.len() {
        5 => Ok(format!("0 {expression}")),
        6 | 7 => Ok(expression.to_owned()),
        _ => anyhow::bail!("cron expression must contain 5, 6, or 7 fields: {expression:?}"),
    }
}

pub fn due_runs(
    trigger: &Trigger,
    scheduled_at: DateTime<Utc>,
    now: DateTime<Utc>,
    misfire: MisfirePolicy,
) -> Result<Vec<DateTime<Utc>>> {
    if scheduled_at > now {
        return Ok(Vec::new());
    }
    match misfire {
        MisfirePolicy::Skip => Ok(Vec::new()),
        MisfirePolicy::RunOnce => Ok(vec![scheduled_at]),
        MisfirePolicy::CatchUp { max_runs: 0 } => {
            anyhow::bail!("catch_up max_runs must be greater than zero")
        }
        MisfirePolicy::CatchUp { max_runs } => {
            let mut runs = Vec::new();
            let mut next = scheduled_at;
            while next <= now && (runs.len() as u32) < max_runs {
                runs.push(next);
                next = next_run(trigger, next)?.unwrap_or(now + Duration::seconds(1));
            }
            Ok(runs)
        }
    }
}

pub fn may_start(policy: ConcurrencyPolicy, running: u32) -> bool {
    match policy {
        ConcurrencyPolicy::Allow => true,
        ConcurrencyPolicy::ForbidOverlap => running == 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn interval_misfire_policies_are_explicit() {
        let trigger = Trigger::Interval { seconds: 60 };
        let scheduled = DateTime::parse_from_rfc3339("2026-08-11T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let now = scheduled + Duration::minutes(3);
        assert!(
            due_runs(&trigger, scheduled, now, MisfirePolicy::Skip)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            due_runs(&trigger, scheduled, now, MisfirePolicy::RunOnce)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            due_runs(
                &trigger,
                scheduled,
                now,
                MisfirePolicy::CatchUp { max_runs: 2 }
            )
            .unwrap()
            .len(),
            2
        );
        assert!(
            due_runs(
                &trigger,
                scheduled,
                now,
                MisfirePolicy::CatchUp { max_runs: 0 }
            )
            .is_err()
        );
    }

    #[test]
    fn forbids_overlap_by_default() {
        assert!(!may_start(ConcurrencyPolicy::ForbidOverlap, 1));
        assert!(may_start(ConcurrencyPolicy::ForbidOverlap, 0));
        assert!(may_start(ConcurrencyPolicy::Allow, 100));
    }

    #[test]
    fn accepts_portable_five_field_cron() {
        let after = DateTime::parse_from_rfc3339("2026-08-11T02:59:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let next = next_run(
            &Trigger::Cron {
                expression: "0 3 * * *".into(),
                timezone: "UTC".into(),
            },
            after,
        )
        .unwrap()
        .unwrap();
        assert_eq!(next.hour(), 3);
        assert_eq!(next.minute(), 0);
    }

    #[test]
    fn rejects_zero_interval() {
        assert!(next_run(&Trigger::Interval { seconds: 0 }, Utc::now()).is_err());
    }

    #[test]
    fn rejects_interval_overflow() {
        assert!(
            next_run(
                &Trigger::Interval {
                    seconds: i64::MAX as u64 + 1,
                },
                Utc::now(),
            )
            .is_err()
        );
        assert!(
            next_run(
                &Trigger::Interval {
                    seconds: i64::MAX as u64,
                },
                Utc::now(),
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_invalid_cron_timezone_during_preflight() {
        assert!(
            next_run(
                &Trigger::Cron {
                    expression: "0 3 * * *".into(),
                    timezone: "Not/A_Timezone".into(),
                },
                Utc::now(),
            )
            .is_err()
        );
    }

    #[test]
    fn evaluates_cron_in_the_declared_iana_timezone() {
        let after = DateTime::parse_from_rfc3339("2026-08-11T06:59:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let next = next_run(
            &Trigger::Cron {
                expression: "0 3 * * *".into(),
                timezone: "America/New_York".into(),
            },
            after,
        )
        .unwrap()
        .unwrap();
        assert_eq!(next.to_rfc3339(), "2026-08-11T07:00:00+00:00");
    }

    #[test]
    fn skips_a_nonexistent_spring_forward_local_time() {
        let after = DateTime::parse_from_rfc3339("2026-03-08T06:59:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let next = next_run(
            &Trigger::Cron {
                expression: "30 2 * * *".into(),
                timezone: "America/New_York".into(),
            },
            after,
        )
        .unwrap()
        .unwrap();
        assert_eq!(next.to_rfc3339(), "2026-03-09T06:30:00+00:00");
    }
}
