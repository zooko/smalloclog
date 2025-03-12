
use std::io::{self, Read};

use smalloclog::smalloclog_to_human_readable;

fn main() {
    let mut buffer = Vec::new();
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    
    if let Err(e) = handle.read_to_end(&mut buffer) {
        eprintln!("Failed to read from stdin: {}", e);
        return;
    }
    
    println!("{}", smalloclog_to_human_readable(&buffer));
}
