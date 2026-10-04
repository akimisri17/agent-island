//! Prints recipe suggestions per repository (counts and the first 60
//! characters only), for checking by hand.
//! cargo run --release --example recipes
use agent_island_lib::{logs, recipes, repos};

fn main() {
    let home = std::path::PathBuf::from(std::env::var_os("HOME").expect("no HOME"));
    let roots = logs::Roots::default_for(&home);
    let t = std::time::Instant::now();
    let root_of = |cwd: &str| repos::root_of(std::path::Path::new(cwd)).map(|p| p.to_string_lossy().into_owned());
    let m = recipes::mine(&roots.claude, logs::now_ms() - 30 * 86_400_000, &root_of);
    eprintln!("{} repos with suggestions in {:.2}s", m.len(), t.elapsed().as_secs_f64());
    for (repo, list) in &m {
        println!("{repo}");
        for s in list {
            println!("  {}×  {}", s.count, s.text.chars().take(60).collect::<String>());
        }
    }
}
