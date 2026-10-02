//! When to notify about limits. At most once per window per kind, so a
//! notification is news, not noise:
//!   Claude close to the 5-hour limit (high risk)   once per window
//!   Claude limit reached                           once per reset time
//!   Claude limit has reset                         once, after a limited window ends
//!   Codex window at 85% or full within 30 minutes  once per window

use crate::limits::{Limits, Risk};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub title: String,
    pub body: String,
}

#[derive(Default)]
pub struct Notifier {
    sent: HashSet<String>,
    /// Reset time of a Claude limit we told the person about.
    limited_until: Option<i64>,
}

fn clock(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.with_timezone(&chrono::Local).format("%-I:%M %p").to_string())
        .unwrap_or_default()
}

impl Notifier {
    pub fn due(&mut self, l: &Limits, now: i64) -> Vec<Note> {
        let mut out = Vec::new();
        if let Some(until) = self.limited_until {
            if now >= until {
                self.limited_until = None;
                out.push(Note { title: "Claude limit has reset".into(), body: "You can start long runs again.".into() });
            }
        }
        if let Some(c) = &l.claude {
            match c.risk {
                Risk::Limited => {
                    if let Some(until) = c.limited_until {
                        self.limited_until = Some(until);
                        if self.sent.insert(format!("claude-limited-{until}")) {
                            out.push(Note { title: "Claude limit reached".into(), body: format!("It resets at {}.", clock(until)) });
                        }
                    }
                }
                Risk::High => {
                    let window = c.window_start.unwrap_or_default();
                    if self.sent.insert(format!("claude-high-{window}")) {
                        let resets = c.window_resets.map(|r| format!(" Resets about {}.", clock(r))).unwrap_or_default();
                        let lead = format!("Past the usage before {} of your {} limit hits.", c.passed, c.past_hits);
                        out.push(Note {
                            title: "Claude: close to your 5-hour limit".into(),
                            body: match &c.advice {
                                Some(a) => format!("{lead} {a}"),
                                None => format!("{lead}{resets}"),
                            },
                        });
                    }
                }
                _ => {}
            }
        }
        for w in &l.codex {
            let soon = w.minutes_to_full.is_some_and(|m| m <= 30.0);
            if w.used_percent < 85.0 && !soon {
                continue;
            }
            if self.sent.insert(format!("codex-{:?}-{:?}", w.window_minutes, w.resets_at)) {
                let name = match w.window_minutes {
                    Some(300) => "5-hour window".to_string(),
                    Some(10080) => "weekly window".to_string(),
                    Some(m) => format!("{}-hour window", m / 60),
                    None => "window".to_string(),
                };
                let eta = w.minutes_to_full.map(|m| format!("Full in about {} min. ", m.round())).unwrap_or_default();
                let reset = w.resets_at.map(|r| format!("Resets {}.", clock(r))).unwrap_or_default();
                out.push(Note { title: format!("Codex {name} at {}%", w.used_percent.round()), body: format!("{eta}{reset}").trim().to_string() });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::{ClaudeLimit, CodexWindow};

    const NOW: i64 = 1_790_000_000_000;
    fn claude(risk: Risk, window_start: i64, limited_until: Option<i64>) -> Limits {
        Limits {
            claude: Some(ClaudeLimit {
                risk,
                limited_until,
                window_start: Some(window_start),
                window_resets: Some(window_start + 5 * 3_600_000),
                passed: 7,
                past_hits: 11,
                sessions_working: 1,
                large_model_share: 0.0,
                advice: None,
            }),
            codex: vec![],
        }
    }

    #[test]
    fn high_risk_once_per_window() {
        let mut n = Notifier::default();
        let l = claude(Risk::High, NOW - 3_600_000, None);
        let first = n.due(&l, NOW);
        assert_eq!(first.len(), 1);
        assert!(first[0].body.starts_with("Past the usage before 7 of your 11"));
        assert!(n.due(&l, NOW + 60_000).is_empty());
        // A new window can notify again.
        assert_eq!(n.due(&claude(Risk::High, NOW + 6 * 3_600_000, None), NOW + 7 * 3_600_000).len(), 1);
    }

    #[test]
    fn limited_then_reset() {
        let mut n = Notifier::default();
        let until = NOW + 3_600_000;
        let l = claude(Risk::Limited, until - 5 * 3_600_000, Some(until));
        assert_eq!(n.due(&l, NOW)[0].title, "Claude limit reached");
        assert!(n.due(&l, NOW + 60_000).is_empty());
        let after = claude(Risk::Low, until + 60_000, None);
        assert_eq!(n.due(&after, until + 60_000)[0].title, "Claude limit has reset");
        assert!(n.due(&after, until + 120_000).is_empty());
    }

    #[test]
    fn codex_near_full_once() {
        let mut n = Notifier::default();
        let l = |used: f64, mins: Option<f64>| Limits {
            claude: None,
            codex: vec![CodexWindow { used_percent: used, window_minutes: Some(300), resets_at: Some(NOW + 3_600_000), minutes_to_full: mins }],
        };
        assert!(n.due(&l(60.0, Some(90.0)), NOW).is_empty());
        let notes = n.due(&l(70.0, Some(25.0)), NOW);
        assert_eq!(notes[0].title, "Codex 5-hour window at 70%");
        assert!(notes[0].body.starts_with("Full in about 25 min."));
        assert!(n.due(&l(90.0, Some(10.0)), NOW).is_empty());
    }
}
