//! Prints the Tasks board, for checking by hand.
//! cargo run --release --example tasks
use agent_island_lib::{logs, tasks};

fn main() {
    let home = std::path::PathBuf::from(std::env::var_os("HOME").expect("no HOME"));
    let roots = logs::Roots::default_for(&home);
    let now = logs::now_ms();
    let t = std::time::Instant::now();
    let defs = tasks::definitions(&home.join(".claude/scheduled-tasks"));
    let runs = tasks::runs(&roots.claude, now - 14 * 86_400_000, now, 5 * 60_000);
    let b = tasks::board(&defs, &runs, now);
    eprintln!("{} tasks, {} runs in {:.2}s", b.len(), runs.len(), t.elapsed().as_secs_f64());
    for r in &b {
        println!("{:<28} {:<8} {:?} cadence {:?}h", r.name, r.state, r.days, r.cadence_ms.map(|c| c / 3_600_000));
    }
}
