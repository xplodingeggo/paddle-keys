mod config;
mod decode;
mod device;
mod keys;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--probe") {
        let secs = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
        if let Err(e) = device::probe(secs) {
            eprintln!("probe failed: {e}");
            std::process::exit(1);
        }
        return;
    }
    let out = keys::Output::new(config::Bindings::default());
    device::run(out, Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
}
