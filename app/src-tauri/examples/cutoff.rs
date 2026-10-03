//! Prints the cut-off sessions for the last 24 hours, for checking by hand.
//! cargo run --release --example cutoff
use agent_island_lib::{cutoff, logs};

fn main() {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).expect("no home dir");
    let roots = logs::Roots::default_for(std::path::Path::new(&home));
    let t = std::time::Instant::now();
    let r = cutoff::find(&roots.claude, logs::now_ms());
    eprintln!("{} cut off in {:.2}s", r.len(), t.elapsed().as_secs_f64());
    println!("{}", serde_json::to_string_pretty(&r).unwrap());
}
