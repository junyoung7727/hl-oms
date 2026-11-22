//! `watcher --version`. The checks, the config and the runtime arrive in
//! U19a5–U19a7; until then every other invocation is refused with exit
//! code 2, without touching the network or a file.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() == 1 && args[0] == "--version" {
        println!("watcher {}", watcher::VERSION);
        return ExitCode::SUCCESS;
    }
    eprintln!("watcher: not built yet: the checks arrive in U19a5–U19a7");
    ExitCode::from(2)
}
