//! Prints the Repo board for the given folders, for checking against git.
//! cargo run --release --example repos -- ~/code/a ~/code/b
use agent_island_lib::repos::status;

fn main() {
    let cwds: Vec<String> = std::env::args().skip(1).collect();
    let t = std::time::Instant::now();
    let r = status(&cwds);
    eprintln!("{} repos in {:.2}s", r.len(), t.elapsed().as_secs_f64());
    println!("{}", serde_json::to_string_pretty(&r).unwrap());
}
