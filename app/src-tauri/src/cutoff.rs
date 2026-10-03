//! Sessions stopped by a usage limit (or interrupted) that nobody went back
//! to: the "Cut off" section of the Waiting tab.

use crate::logs::{is_human_prompt, project_of, CLine};
use serde::Serialize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const WINDOW_MS: i64 = 24 * 3_600_000;
/// Only the end of each log is read: the events that matter are the last ones.
const TAIL_BYTES: u64 = 256 * 1024;
const INTERRUPTED: [&str; 2] = ["[Request interrupted by user]", "[Request interrupted by user for tool use]"];

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CutOff {
    pub session_id: String,
    pub title: Option<String>,
    pub project: Option<String>,
    pub cwd: String,
    /// "limit" or "interrupted".
    pub kind: &'static str,
    /// e.g. "five_hour", for limit cut-offs.
    pub limit_type: Option<String>,
    /// When it was cut off, ms.
    pub at: i64,
    /// When the limit reset, ms (limit cut-offs only).
    pub resets_at: Option<i64>,
}

type Cut = (String, i64, &'static str, Option<String>, Option<i64>);

/// What the end of one log says.
struct Tail {
    /// Programmatic (Agent SDK / `claude -p`) session: nobody typed in it.
    sdk: bool,
    path: PathBuf,
    mtime: i64,
    session_id: String,
    title: Option<String>,
    cwd: Option<String>,
    last_human: Option<i64>,
    /// The last cut-off event: (uuid, at, kind, limit type, resets at).
    cut: Option<Cut>,
}

fn read_tail(path: &Path) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    f.read_to_end(&mut buf).ok()?;
    let mut s = String::from_utf8_lossy(&buf).into_owned();
    if start > 0 {
        // The first line is probably cut in half.
        s = s.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default();
    }
    Some(s)
}

fn block_text(d: &CLine) -> Option<String> {
    let raw = d.message.as_ref()?.content?;
    if raw.get().starts_with('"') {
        return serde_json::from_str::<String>(raw.get()).ok();
    }
    let blocks: Vec<crate::logs::Block> = serde_json::from_str(raw.get()).ok()?;
    blocks.into_iter().find(|b| b.kind.as_deref() == Some("text")).and_then(|b| b.text)
}

fn parse_ts(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis())
}

fn tail_of(path: &Path, mtime: i64) -> Option<Tail> {
    let text = read_tail(path)?;
    let session_id = path.file_stem()?.to_string_lossy().into_owned();
    let mut t = Tail { sdk: false, path: path.to_path_buf(), mtime, session_id, title: None, cwd: None, last_human: None, cut: None };
    for line in text.lines() {
        let Ok(mut d) = serde_json::from_str::<CLine>(line) else { continue };
        if d.kind == Some("custom-title") {
            t.title = d.custom_title.take().filter(|s| !s.is_empty()).or(t.title.take());
        }
        if d.kind == Some("agent-name") && t.title.is_none() {
            t.title = d.agent_name.take().filter(|s| !s.is_empty());
        }
        let Some(at) = d.timestamp.and_then(parse_ts) else { continue };
        if d.cwd.is_some() {
            t.cwd = d.cwd.clone();
        }
        let uuid = d.uuid.clone().unwrap_or_default();
        if let Some(q) = d.quota_limits.as_ref().filter(|q| q.status.as_deref() == Some("rejected")) {
            let resets = q.resets_at.map(|s| (s * 1000.0) as i64);
            t.cut = Some((uuid, at, "limit", q.rate_limit_type.clone(), resets));
            continue;
        }
        if d.kind == Some("assistant") && d.message.as_ref().and_then(|m| m.model.as_deref()) != Some("<synthetic>") {
            // A real reply: the session carried on after the cut.
            t.cut = None;
        }
        if d.kind == Some("user") && d.entrypoint.is_some_and(|e| e.starts_with("sdk")) {
            t.sdk = true;
        }
        if d.kind == Some("user") && d.is_sidechain != Some(true) {
            if block_text(&d).is_some_and(|x| INTERRUPTED.contains(&x.trim())) {
                t.cut = Some((uuid, at, "interrupted", None, None));
            } else if is_human_prompt(&d) {
                t.last_human = Some(at);
            }
        }
    }
    Some(t)
}

fn recent_logs(dir: &Path, since: i64, out: &mut Vec<(PathBuf, i64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            if p.file_name().is_some_and(|n| n == "subagents") {
                continue;
            }
            recent_logs(&p, since, out);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            let mtime = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64);
            if mtime >= since {
                out.push((p, mtime));
            }
        }
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Cut-off sessions under `projects` (~/.claude/projects) as of `now` (ms),
/// newest first.
pub fn find(projects: &Path, now: i64) -> Vec<CutOff> {
    let since = now - WINDOW_MS;
    let mut files = Vec::new();
    recent_logs(projects, since, &mut files);
    let tails: Vec<Tail> = files.iter().filter_map(|(p, m)| tail_of(p, *m)).collect();

    let mut out = Vec::new();
    for t in tails.iter().filter(|t| !t.sdk) {
        let Some((uuid, at, kind, limit_type, resets_at)) = &t.cut else { continue };
        if *at < since || t.last_human.is_some_and(|h| h > *at) {
            continue;
        }
        if resets_at.is_some_and(|r| r > now) {
            continue;
        }
        // Resumed or forked: another log copied this line and went on.
        let needle = format!("\"uuid\":\"{uuid}\"");
        let resolved = !uuid.is_empty() && tails.iter().any(|o| {
            o.path != t.path
                && o.mtime >= *at
                && o.last_human.is_some_and(|h| h > *at)
                && std::fs::read(&o.path).is_ok_and(|b| contains(&b, needle.as_bytes()))
        });
        if resolved {
            continue;
        }
        let cwd = t.cwd.clone().unwrap_or_default();
        out.push(CutOff {
            session_id: t.session_id.clone(),
            title: t.title.clone(),
            project: project_of(&cwd),
            cwd,
            kind,
            limit_type: limit_type.clone(),
            at: *at,
            resets_at: *resets_at,
        });
    }
    out.sort_by_key(|c| std::cmp::Reverse(c.at));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const NOW: i64 = 1_790_000_000_000; // ms
    fn ts(ms: i64) -> String {
        chrono::DateTime::from_timestamp_millis(ms).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }
    fn human(uuid: &str, at: i64, text: &str) -> String {
        format!(r#"{{"type":"user","uuid":"{uuid}","timestamp":"{}","sessionId":"s","cwd":"/w/shop","origin":{{"kind":"human"}},"message":{{"role":"user","content":"{text}"}}}}"#, ts(at))
    }
    fn assistant(uuid: &str, at: i64) -> String {
        format!(r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"{}","sessionId":"s","cwd":"/w/shop","message":{{"id":"m{uuid}","model":"claude-opus-5","content":[{{"type":"text","text":"ok"}}]}}}}"#, ts(at))
    }
    fn limit(uuid: &str, at: i64, resets_s: i64) -> String {
        format!(r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"{}","sessionId":"s","cwd":"/w/shop","isApiErrorMessage":true,"error":"rate_limit","quotaLimits":{{"status":"rejected","resetsAt":{resets_s},"rateLimitType":"five_hour"}},"message":{{"id":"x","model":"<synthetic>","content":[{{"type":"text","text":"You've hit your session limit"}}]}}}}"#, ts(at))
    }
    fn interrupted(uuid: &str, at: i64) -> String {
        format!(r#"{{"type":"user","uuid":"{uuid}","timestamp":"{}","sessionId":"s","cwd":"/w/shop","message":{{"role":"user","content":[{{"type":"text","text":"[Request interrupted by user]"}}]}}}}"#, ts(at))
    }
    fn title(t: &str) -> String {
        format!(r#"{{"type":"custom-title","customTitle":"{t}","sessionId":"s"}}"#)
    }
    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ai-cutoff-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("proj")).unwrap();
        d
    }
    fn write(d: &std::path::Path, name: &str, lines: &[String]) {
        let mut f = std::fs::File::create(d.join("proj").join(name)).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }
    const H: i64 = 3_600_000;

    #[test]
    fn limit_after_reset_is_cut_off() {
        let d = dir("limit");
        write(&d, "aaaa-1111.jsonl", &[title("Repo board"), human("u1", NOW - 3 * H, "go"), assistant("a1", NOW - 3 * H + 1000), limit("l1", NOW - 2 * H, (NOW - H) / 1000)]);
        let r = find(&d, NOW);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].session_id, "aaaa-1111");
        assert_eq!(r[0].kind, "limit");
        assert_eq!(r[0].limit_type.as_deref(), Some("five_hour"));
        assert_eq!(r[0].title.as_deref(), Some("Repo board"));
        assert_eq!(r[0].cwd, "/w/shop");
        assert_eq!(r[0].project.as_deref(), Some("shop"));
        assert_eq!(r[0].at, NOW - 2 * H);
        assert_eq!(r[0].resets_at, Some(NOW - H));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn limit_not_yet_reset_waits() {
        let d = dir("pending");
        write(&d, "bbbb.jsonl", &[human("u1", NOW - H, "go"), limit("l1", NOW - H / 2, (NOW + H) / 1000)]);
        assert!(find(&d, NOW).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_prompt_after_the_limit_means_you_went_back() {
        let d = dir("back");
        write(&d, "cccc.jsonl", &[human("u1", NOW - 3 * H, "go"), limit("l1", NOW - 2 * H, (NOW - H) / 1000), human("u2", NOW - H / 2, "Try again")]);
        assert!(find(&d, NOW).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn interrupted_and_left_is_cut_off() {
        let d = dir("int");
        write(&d, "dddd.jsonl", &[human("u1", NOW - 2 * H, "go"), assistant("a1", NOW - 2 * H + 5000), interrupted("i1", NOW - 2 * H + 9000)]);
        let r = find(&d, NOW);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].kind, "interrupted");
        assert_eq!(r[0].resets_at, None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn older_than_a_day_is_ignored() {
        let d = dir("old");
        write(&d, "eeee.jsonl", &[human("u1", NOW - 30 * H, "go"), limit("l1", NOW - 26 * H, (NOW - 25 * H) / 1000)]);
        assert!(find(&d, NOW).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn resumed_in_a_new_file_is_resolved() {
        let d = dir("fork");
        let orig = [human("u1", NOW - 3 * H, "go"), limit("l1", NOW - 2 * H, (NOW - H) / 1000)];
        write(&d, "ffff-orig.jsonl", &orig);
        let mut copy = orig.to_vec();
        copy.push(human("u9", NOW - H / 2, "continue"));
        write(&d, "ffff-copy.jsonl", &copy);
        assert!(find(&d, NOW).is_empty(), "the copy has a prompt after the cut-off line");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn sdk_sessions_are_ignored() {
        let d = dir("sdk");
        let sdk_user = format!(r#"{{"type":"user","uuid":"u1","timestamp":"{}","sessionId":"s","cwd":"/w/shop","entrypoint":"sdk-cli","message":{{"role":"user","content":"go"}}}}"#, ts(NOW - 3 * H));
        write(&d, "hhhh.jsonl", &[sdk_user, limit("l1", NOW - 2 * H, (NOW - H) / 1000)]);
        assert!(find(&d, NOW).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_real_reply_after_the_limit_clears_it() {
        let d = dir("reply");
        write(&d, "iiii.jsonl", &[human("u1", NOW - 3 * H, "go"), limit("l1", NOW - 2 * H, (NOW - H) / 1000), assistant("a2", NOW - H / 2)]);
        assert!(find(&d, NOW).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn subagent_logs_are_ignored() {
        let d = dir("sub");
        std::fs::create_dir_all(d.join("proj/gggg/subagents")).unwrap();
        write(&d, "gggg/subagents/agent-1.jsonl", &[human("u1", NOW - 3 * H, "go"), limit("l1", NOW - 2 * H, (NOW - H) / 1000)]);
        assert!(find(&d, NOW).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
