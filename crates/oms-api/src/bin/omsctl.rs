//! `omsctl`: command-line client for the OMS local sockets.
//!
//! ```text
//! omsctl --socket <path> [--strategy <id>] <command> [options]
//!
//! strategy commands (trading socket, --strategy required):
//!   new --key K --instrument I --side buy|sell --qty Q --px P
//!       [--tif gtc|ioc|alo] [--ttl-ms 5000] [--reduce-only]
//!   cancel --order N | --key K
//!   get --order N | --key K
//!   open
//!   watch
//!   status
//!
//! operator commands (admin socket, no --strategy):
//!   status | open | get --order N | cancel --order N
//!   halt [--target-strategy S] --note TEXT
//!   resume [--target-strategy S] --note TEXT
//!   cancel-all [--target-strategy S] --note TEXT
//!   ack --id N --note TEXT
//!   resolve --order N --note TEXT
//!   adjust --instrument I --delta D [--target-strategy S] --note TEXT
//!   snapshot
//! ```
//!
//! Every operator command is recorded in the journal with its note.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match oms_api::ctl::run(&args) {
        Ok(text) => println!("{text}"),
        Err(e) => {
            eprintln!("omsctl: {e}");
            std::process::exit(1);
        }
    }
}
