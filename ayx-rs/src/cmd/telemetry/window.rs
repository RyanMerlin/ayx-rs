//! `--since` time-window parsing.
//!
//! Accepts `<N><unit>` where unit ∈ {h, d, w}. Returns the cutoff as a
//! UTC `DateTime` along with the canonical string for echo in envelopes.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};

#[derive(Debug, Clone)]
pub struct Window {
    pub label: String,
    pub since: DateTime<Utc>,
    #[allow(dead_code)] // exposed for Phase 2 server timeline endpoints.
    pub now: DateTime<Utc>,
}

impl Window {
    pub fn parse(s: &str) -> Result<Self> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(anyhow!(
                "validation: --since cannot be empty; expected e.g. 24h, 7d, 4w"
            ));
        }
        let (num_part, unit) = split_num_unit(trimmed).ok_or_else(|| {
            anyhow!("validation: invalid --since '{trimmed}'; expected <N>{{h,d,w}}")
        })?;
        let n: i64 = num_part
            .parse()
            .map_err(|_| anyhow!("validation: --since count '{num_part}' is not an integer"))?;
        if n <= 0 {
            return Err(anyhow!(
                "validation: --since count must be positive; got {n}"
            ));
        }
        let dur = match unit {
            'h' | 'H' => Duration::hours(n),
            'd' | 'D' => Duration::days(n),
            'w' | 'W' => Duration::weeks(n),
            other => {
                return Err(anyhow!(
                    "validation: unknown --since unit '{other}'; expected one of h, d, w"
                ))
            }
        };
        let now = Utc::now();
        Ok(Self {
            label: trimmed.to_ascii_lowercase(),
            since: now - dur,
            now,
        })
    }
}

fn split_num_unit(s: &str) -> Option<(&str, char)> {
    let last = s.chars().last()?;
    if !last.is_ascii_alphabetic() {
        return None;
    }
    let split_at = s.len() - last.len_utf8();
    Some((&s[..split_at], last))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hours_days_weeks() {
        let w = Window::parse("24h").unwrap();
        assert_eq!(w.label, "24h");
        assert!((w.now - w.since).num_hours() == 24);

        let w = Window::parse("7d").unwrap();
        assert_eq!((w.now - w.since).num_days(), 7);

        let w = Window::parse("4w").unwrap();
        assert_eq!((w.now - w.since).num_weeks(), 4);
    }

    #[test]
    fn rejects_bad_inputs() {
        assert!(Window::parse("").is_err());
        assert!(Window::parse("d").is_err());
        assert!(Window::parse("7").is_err());
        assert!(Window::parse("7x").is_err());
        assert!(Window::parse("0d").is_err());
        assert!(Window::parse("-1d").is_err());
    }
}
