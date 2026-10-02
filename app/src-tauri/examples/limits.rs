//! Prints the limit coach's view of this machine.
//! cargo run --release --example limits
use agent_island_lib::{limits, logs};

fn main() {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).expect("no home dir");
    let roots = logs::Roots::default_for(std::path::Path::new(&home));
    let now = logs::now_ms();
    let t0 = std::time::Instant::now();
    let (spend, hits) = limits::read_claude(&roots.claude, now - 35 * 86_400_000);
    let l = limits::claude_limit(&spend, &hits, now, 1);
    println!("{}", serde_json::to_string_pretty(&l).unwrap());
    println!("codex: {}", serde_json::to_string(&limits::codex_limits(&roots.codex, now)).unwrap());
    eprintln!("{} messages, {} limit hits, {:.1}s", spend.len(), hits.len(), t0.elapsed().as_secs_f64());
}
