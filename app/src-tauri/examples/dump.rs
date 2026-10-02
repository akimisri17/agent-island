//! Dumps a scan as JSON, for checking the Rust parser against the JS one.
//! cargo run --release --example dump -- 30 > scan.json
use agent_island_lib::logs::{scan, Roots};

fn main() {
    let days = std::env::args().nth(1).and_then(|d| d.parse().ok()).unwrap_or(30);
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).expect("no home dir");
    let r = scan(&Roots::default_for(std::path::Path::new(&home)), days);
    eprintln!("{} sessions, {} files, {:.2} GB in {:.1}s", r.sessions.len(), r.files.claude + r.files.codex, r.bytes as f64 / 1e9, r.seconds);
    println!("{}", serde_json::to_string(&r).unwrap());
}
