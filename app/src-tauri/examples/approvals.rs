//! Prints approval friction per project (counts and command groups only).
//! cargo run --release --example approvals -- 30
use agent_island_lib::{approvals, logs, perms, repos};

fn main() {
    let days: i64 = std::env::args().nth(1).and_then(|d| d.parse().ok()).unwrap_or(30);
    let home = std::path::PathBuf::from(std::env::var_os("HOME").expect("no HOME"));
    let roots = logs::Roots::default_for(&home);
    let t = std::time::Instant::now();
    let root_of = |cwd: &str| repos::root_of(std::path::Path::new(cwd)).map(|p| p.to_string_lossy().into_owned());
    let rules_for = |p: &str| perms::Rules::for_project(std::path::Path::new(p), &home);
    let r = approvals::scan(&roots.claude, logs::now_ms() - days * 86_400_000, &root_of, &rules_for);
    eprintln!("{} projects in {:.2}s", r.len(), t.elapsed().as_secs_f64());
    for p in r.iter().take(8) {
        println!("{:<28} asked {:>4}  wait {:?}s  yes {:>3}  {:?}  -> {:?}", p.name, p.asked, p.median_wait_ms.map(|w| w / 1000), p.affirmations, p.commands.iter().take(3).collect::<Vec<_>>(), p.suggestions);
    }
}
