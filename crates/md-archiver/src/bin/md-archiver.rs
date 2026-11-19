//! `md-archiver`: builds the L1 tables of a venue from its sealed L0 files
//! (lake contract 3, spec D1).
//!
//! ```text
//! md-archiver version
//! ```

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("version") => {
            println!(
                "md-archiver {} parser_ver {}",
                env!("CARGO_PKG_VERSION"),
                md_archiver::PARSER_VER_BYBIT
            );
            0
        }
        _ => {
            eprintln!("usage:\n  md-archiver version");
            2
        }
    };
    std::process::exit(code);
}
