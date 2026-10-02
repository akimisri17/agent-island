//! Prints the running sessions and their state, longest wait first.
//! cargo run --example live
use agent_island_lib::{live::live_sessions, logs::Roots};

fn main() {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).expect("no home dir");
    let t0 = std::time::Instant::now();
    let sessions = live_sessions(&Roots::default_for(std::path::Path::new(&home)));
    let now = agent_island_lib::logs::now_ms();
    for s in &sessions {
        println!(
            "{:<9} {:>4} min  {:<7} {:<10} {:<22} {}",
            format!("{:?}", s.state).to_lowercase(),
            (now - s.since) / 60_000,
            s.agent,
            s.host.as_deref().unwrap_or("?"),
            s.project.as_deref().unwrap_or("?"),
            s.title.as_deref().unwrap_or("")
        );
    }
    eprintln!("{} live sessions in {:.0} ms", sessions.len(), t0.elapsed().as_secs_f64() * 1000.0);
}
