use std::io::{self, Read};

use smalloclog::smalloclog_to_stats;

//XXXuse firestorm::profile_fn;

fn get_and_print() -> () {
//XXX    let mut buffer: VecDeque<u8> = VecDeque::with_capacity(2usize.pow(20));
    let mut buffer = Vec::new();

//XXX    let mut buffer: VecDeque<u8> = VecDeque::with_capacity(2usize.pow(20));

    let stdin = io::stdin();
    let mut handle = stdin.lock();

//XXX    loop {
//XXX	buffer.extend_from_slice(xxx);
//XXX    }
    
    if let Err(e) = handle.read_to_end(&mut buffer) {
        eprintln!("Failed to read from stdin: {}", e);
        return;
    }
    
    println!("{}", smalloclog_to_stats(&buffer));
}


fn main() {
    get_and_print();
}

//XXX#[async_std::main]
//XXXfn main() -> std::io::Result<()> {
//XXX};
