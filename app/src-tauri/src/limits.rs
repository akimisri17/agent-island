//! Limit coach: where you stand against your usage limits, said only as far
//! as local data supports it.
//!
//! Claude: logs record only the moment a limit is hit ("quotaLimits" with
//! status "rejected" and a reset time), never a running percentage. The
//! 5-hour limit is also shared with claude.ai and desktop chats, which leave
//! no local trace, so a percentage would be invented. Instead this reports:
//!   - when you are limited, the exact reset time Claude logged;
//!   - how this window's usage compares with the usage that preceded your
//!     past limit hits ("past the usage before 7 of your 12 hits");
//!   - one suggested move.
//! Codex: rollout logs carry official used_percent and reset times per
//! window, so those are shown as-is, with a forecast from their slope.

use serde::Serialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const HOUR: i64 = 3_600_000;
const WINDOW: i64 = 5 * HOUR;
const PACE_SPAN: i64 = 30 * 60_000;

#[derive(Serialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Unknown,
    Low,
    Medium,
    High,
    Limited,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeLimit {
    pub risk: Risk,
    /// Set while a 5-hour limit is in force: the reset time Claude logged.
    pub limited_until: Option<i64>,
    /// Estimated start and reset of the current 5-hour window.
    pub window_start: Option<i64>,
    pub window_resets: Option<i64>,
    /// Past limit hits whose preceding usage this window has already passed.
    pub passed: usize,
    pub past_hits: usize,
    pub sessions_working: usize,
    pub large_model_share: f64,
    pub advice: Option<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CodexWindow {
    pub used_percent: f64,
    pub window_minutes: Option<i64>,
    pub resets_at: Option<i64>,
    /// Minutes until 100% at the recent pace, when it is rising.
    pub minutes_to_full: Option<f64>,
}

#[derive(Serialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub claude: Option<ClaudeLimit>,
    pub codex: Vec<CodexWindow>,
}

/// One assistant message: when, how much it spent, whether a large model.
#[derive(Debug, Clone, Copy)]
pub struct Spend {
    pub ts: i64,
    pub units: f64,
    pub large: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Hit {
    pub ts: i64,
    pub resets_at: i64,
}

/// Spend is a rough cost proxy: cache reads are cheap, output is dear, and
/// larger models cost more. It is only ever compared with itself.
fn units(model: &str, u: &serde_json::Value) -> (f64, bool) {
    let n = |k: &str| u.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let tokens = n("input_tokens") + 1.25 * n("cache_creation_input_tokens") + 0.1 * n("cache_read_input_tokens") + 5.0 * n("output_tokens");
    let m = model.to_ascii_lowercase();
    let (weight, large) = if ["haiku", "mini", "flash"].iter().any(|s| m.contains(s)) {
        (0.33, false)
    } else if ["opus", "fable"].iter().any(|s| m.contains(s)) {
        (5.0, true)
    } else {
        (1.0, false)
    };
    (weight * tokens, large)
}

fn ts(v: &serde_json::Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(v.get("timestamp")?.as_str()?).ok().map(|d| d.timestamp_millis())
}

/// Spend and 5-hour limit hits from Claude logs modified since `since`.
pub fn read_claude(projects: &Path, since: i64) -> (Vec<Spend>, Vec<Hit>) {
    use rayon::prelude::*;
    let mut files = Vec::new();
    collect(projects, since, &mut files);
    let parts: Vec<_> = files.par_iter().map(|f| read_claude_file(f, since)).collect();
    let mut by_id: HashMap<String, Spend> = HashMap::new();
    let mut hits: HashMap<i64, i64> = HashMap::new(); // resets_at -> first seen
    for (spend, file_hits) in parts {
        by_id.extend(spend);
        for (r, t) in file_hits {
            let e = hits.entry(r).or_insert(t);
            *e = (*e).min(t);
        }
    }
    let mut spend: Vec<Spend> = by_id.into_values().collect();
    spend.sort_by_key(|s| s.ts);
    let mut hits: Vec<Hit> = hits.into_iter().map(|(resets_at, ts)| Hit { ts, resets_at }).collect();
    hits.sort_by_key(|h| h.ts);
    (spend, hits)
}

type FileSpend = (HashMap<String, Spend>, HashMap<i64, i64>);

fn read_claude_file(f: &Path, since: i64) -> FileSpend {
    let mut by_id: HashMap<String, Spend> = HashMap::new();
    let mut hits: HashMap<i64, i64> = HashMap::new();
    {
        let Ok(file) = std::fs::File::open(f) else { return (by_id, hits) };
        for line in BufReader::with_capacity(1 << 20, file).lines().map_while(Result::ok) {
            let is_hit = line.contains("\"quotaLimits\"");
            if !is_hit && !line.contains("\"type\":\"assistant\"") {
                continue;
            }
            let Ok(d) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
            let Some(t) = ts(&d).filter(|&t| t >= since) else { continue };
            if let Some(q) = d.get("quotaLimits").filter(|q| q.get("status").and_then(|s| s.as_str()) == Some("rejected")) {
                if q.get("rateLimitType").and_then(|s| s.as_str()) == Some("five_hour") {
                    if let Some(r) = q.get("resetsAt").and_then(|r| r.as_f64()) {
                        let e = hits.entry((r * 1000.0) as i64).or_insert(t);
                        *e = (*e).min(t);
                    }
                }
            }
            if d.get("type").and_then(|s| s.as_str()) != Some("assistant") {
                continue;
            }
            let (Some(id), Some(u)) = (d.pointer("/message/id").and_then(|s| s.as_str()), d.pointer("/message/usage")) else { continue };
            let (units, large) = units(d.pointer("/message/model").and_then(|s| s.as_str()).unwrap_or(""), u);
            by_id.insert(id.to_string(), Spend { ts: t, units, large });
        }
    }
    (by_id, hits)
}

fn collect(dir: &Path, since: i64, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, since, out);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            let fresh = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .is_some_and(|d| d.as_millis() as i64 >= since);
            if fresh {
                out.push(p);
            }
        }
    }
}

/// Start of the 5-hour window containing `now`: a window opens with the
/// first message after the previous one closed. A logged reset pins it.
fn window_start(spend: &[Spend], hits: &[Hit], now: i64) -> Option<i64> {
    if let Some(h) = hits.iter().rev().find(|h| h.resets_at > now) {
        return Some(h.resets_at - WINDOW);
    }
    let mut start: Option<i64> = None;
    for s in spend.iter().filter(|s| s.ts <= now) {
        match start {
            Some(st) if s.ts < st + WINDOW => {}
            _ => start = Some(s.ts),
        }
    }
    start.filter(|&st| now < st + WINDOW)
}

pub fn claude_limit(spend: &[Spend], hits: &[Hit], now: i64, sessions_working: usize) -> ClaudeLimit {
    let limited_until = hits.iter().rev().find(|h| h.resets_at > now && h.ts <= now).map(|h| h.resets_at);
    let start = window_start(spend, hits, now);
    let in_window: Vec<&Spend> = spend.iter().filter(|s| start.is_some_and(|st| s.ts >= st && s.ts <= now)).collect();
    let used: f64 = in_window.iter().map(|s| s.units).sum();
    let large = in_window.iter().filter(|s| s.large).map(|s| s.units).sum::<f64>();
    let large_model_share = if used > 0.0 { large / used } else { 0.0 };

    // Usage that preceded each past hit, within that hit's window.
    let past: Vec<f64> = hits
        .iter()
        .filter(|h| h.resets_at <= now)
        .map(|h| spend.iter().filter(|s| s.ts >= h.resets_at - WINDOW && s.ts <= h.ts).map(|s| s.units).sum())
        .filter(|&u: &f64| u > 0.0)
        .collect();
    let passed = past.iter().filter(|&&p| used >= p).count();

    let risk = if limited_until.is_some() {
        Risk::Limited
    } else if past.is_empty() {
        Risk::Unknown
    } else if passed * 2 >= past.len() {
        Risk::High
    } else if passed > 0 {
        Risk::Medium
    } else {
        Risk::Low
    };
    let window_resets = start.map(|s| s + WINDOW);
    let recent: f64 = in_window.iter().filter(|s| s.ts >= now - PACE_SPAN).map(|s| s.units).sum();
    let busy = recent > 0.0;

    // One move. The panel shows the times and the hit count itself.
    let advice = match risk {
        Risk::Limited => Some("Queue long runs for after the reset.".to_string()),
        Risk::High if sessions_working >= 2 && busy => Some(format!("{sessions_working} sessions are working. Pausing one buys the most time.")),
        Risk::High if large_model_share >= 0.5 && busy => {
            Some("Mostly Opus-class models this window. A smaller model for the next stretch spends far less.".to_string())
        }
        Risk::High => Some("Close to where past limits hit. Save long runs for after the reset.".to_string()),
        Risk::Medium | Risk::Low | Risk::Unknown => None,
    };

    ClaudeLimit {
        risk,
        limited_until,
        window_start: start,
        window_resets,
        passed,
        past_hits: past.len(),
        sessions_working,
        large_model_share,
        advice,
    }
}

/// Official Codex windows from the newest rollout log, with a forecast from
/// how fast used_percent rose over the last hour.
pub fn codex_limits(sessions_root: &Path, now: i64) -> Vec<CodexWindow> {
    let mut files = Vec::new();
    collect(sessions_root, now - 7 * 24 * HOUR, &mut files);
    let newest = files.into_iter().max_by_key(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok());
    let Some(newest) = newest else { return Vec::new() };
    let Ok(text) = std::fs::read_to_string(newest) else { return Vec::new() };
    codex_from_log(&text, now)
}

pub fn codex_from_log(text: &str, now: i64) -> Vec<CodexWindow> {
    // (ts, [(used, window_minutes, resets_at); primary, secondary])
    let mut samples: Vec<(i64, [Option<(f64, Option<i64>, Option<i64>)>; 2])> = Vec::new();
    for line in text.lines() {
        if !line.contains("\"rate_limits\"") {
            continue;
        }
        let Ok(d) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let Some(t) = ts(&d) else { continue };
        let Some(rl) = d.pointer("/payload/rate_limits") else { continue };
        let w = |k: &str| {
            let x = rl.get(k)?;
            Some((
                x.get("used_percent")?.as_f64()?,
                x.get("window_minutes").and_then(|v| v.as_i64()),
                x.get("resets_at").and_then(|v| v.as_f64()).map(|r| (r * 1000.0) as i64),
            ))
        };
        samples.push((t, [w("primary"), w("secondary")]));
    }
    let Some((_, last)) = samples.last() else { return Vec::new() };
    (0..2)
        .filter_map(|i| {
            let (used, window_minutes, resets_at) = last[i]?;
            if resets_at.is_some_and(|r| r <= now) {
                return None; // that window already reset
            }
            // Slope over the last hour, same window only.
            let first = samples.iter().find(|(t, w)| *t >= now - HOUR && w[i].is_some_and(|x| x.2 == resets_at)).and_then(|(t, w)| Some((*t, w[i]?.0)));
            let minutes_to_full = first.and_then(|(t0, u0)| {
                let mins = (samples.last()?.0 - t0) as f64 / 60_000.0;
                let rate = (used - u0) / mins;
                (mins >= 5.0 && rate > 0.0).then(|| (100.0 - used) / rate)
            });
            Some(CodexWindow { used_percent: used, window_minutes, resets_at, minutes_to_full })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_790_000_000_000;
    fn spend(ago_min: i64, units: f64, large: bool) -> Spend {
        Spend { ts: NOW - ago_min * 60_000, units, large }
    }

    #[test]
    fn units_weigh_output_and_model_size() {
        let u = serde_json::json!({"input_tokens": 100, "cache_read_input_tokens": 1000, "output_tokens": 10});
        assert_eq!(units("claude-sonnet-5", &u), (250.0, false));
        assert_eq!(units("claude-opus-5", &u), (1250.0, true));
        assert_eq!(units("claude-haiku-4-5", &u).1, false);
    }

    #[test]
    fn window_opens_on_first_message_after_the_last_closed() {
        // 400 and 200 minutes ago share a window (it closed 100 minutes ago);
        // the message 30 minutes ago opened the current one.
        let s = [spend(400, 1.0, false), spend(200, 1.0, false), spend(30, 1.0, false)];
        assert_eq!(window_start(&s, &[], NOW), Some(NOW - 30 * 60_000));
        let s = [spend(250, 1.0, false), spend(30, 1.0, false)];
        assert_eq!(window_start(&s, &[], NOW), Some(NOW - 250 * 60_000));
        assert_eq!(window_start(&[spend(400, 1.0, false)], &[], NOW), None);
    }

    #[test]
    fn limited_shows_exact_reset() {
        let hit = Hit { ts: NOW - 60_000, resets_at: NOW + 2 * HOUR };
        let l = claude_limit(&[spend(10, 5.0, false)], &[hit], NOW, 1);
        assert_eq!(l.risk, Risk::Limited);
        assert_eq!(l.limited_until, Some(NOW + 2 * HOUR));
        assert_eq!(l.window_start, Some(NOW + 2 * HOUR - WINDOW));
        assert!(l.advice.unwrap().contains("after the reset"));
    }

    #[test]
    fn compares_with_usage_before_past_hits() {
        // Two past windows that hit the limit after 10 and 40 units.
        let day = 24 * 60;
        let mut s = vec![spend(2 * day, 10.0, false), spend(day, 40.0, false)];
        let hits = [
            Hit { ts: NOW - 2 * day * 60_000, resets_at: NOW - 2 * day * 60_000 + HOUR },
            Hit { ts: NOW - day * 60_000, resets_at: NOW - day * 60_000 + HOUR },
        ];
        s.push(spend(60, 15.0, true));
        let l = claude_limit(&s, &hits, NOW, 1);
        assert_eq!((l.passed, l.past_hits), (1, 2));
        assert_eq!(l.risk, Risk::High);
        // Nothing spent in the last 30 minutes: no parallel or model advice.
        assert!(l.advice.as_deref().unwrap().starts_with("Close to where past limits hit"));
        s.push(spend(10, 30.0, true));
        let l = claude_limit(&s, &hits, NOW, 3);
        assert_eq!(l.passed, 2);
        assert!(l.advice.as_deref().unwrap().starts_with("3 sessions are working"));
        let l = claude_limit(&s, &hits, NOW, 1);
        assert!(l.advice.as_deref().unwrap().contains("Opus-class"));
        assert_eq!(claude_limit(&[spend(10, 1.0, false)], &[], NOW, 1).risk, Risk::Unknown);
    }

    #[test]
    fn codex_official_windows_and_forecast() {
        let line = |min_ago: i64, used: f64| {
            let t = chrono::DateTime::from_timestamp_millis(NOW - min_ago * 60_000).unwrap().to_rfc3339();
            format!(
                r#"{{"timestamp":"{t}","type":"event_msg","payload":{{"type":"token_count","rate_limits":{{"primary":{{"used_percent":{used},"window_minutes":300,"resets_at":{}}}}}}}}}"#,
                (NOW + HOUR) / 1000
            )
        };
        let log = [line(50, 40.0), line(20, 55.0), line(0, 70.0)].join("\n");
        let w = codex_from_log(&log, NOW);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].used_percent, 70.0);
        assert_eq!(w[0].window_minutes, Some(300));
        // 30 points in 50 minutes -> 30 left takes 50 minutes.
        assert!((w[0].minutes_to_full.unwrap() - 50.0).abs() < 0.1);
    }
}
