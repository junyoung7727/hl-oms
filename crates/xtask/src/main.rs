//! Project automation. Run as `cargo run -p xtask -- <command>`.
//!
//! ```text
//! fsync-probe --dir <dir> [--count N] [--bytes N]   flush latency of a directory's device
//! core-bench [--orders N]                           decide+apply and record encoding cost
//! digest                                            source digest, as embedded in oms-node
//! verify [--quick] [--live-testnet]                 run the acceptance suite, write evidence/latest
//! docs                                              validate the registry, generate tables and dashboard data
//! mutants [--only M03]                              selective mutation check, write evidence/mutants.json
//! recovery-time [--dir D] [--orders N]              journal replay time after a kill, write evidence/perf/recovery.json
//! perf [--dir D] [--rates 20,200,2000] [--seconds N] [--connections N] [--inline] [--label L]
//!                                                   end-to-end latency under open-loop load
//! ```

mod bench;
mod digest;
mod layers;
mod loadgen;
mod mutants;
mod perf;
mod registry;
mod verify;

fn opt(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn num(args: &[String], name: &str, default: u64) -> u64 {
    opt(args, name).and_then(|s| s.parse().ok()).unwrap_or(default)
}

/// The workspace root, wherever the command is run from.
fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("fsync-probe") => match opt(&args, "--dir") {
            Some(dir) => bench::fsync_probe(&dir, num(&args, "--count", 300), num(&args, "--bytes", 300), opt(&args, "--label").as_deref()),
            None => {
                eprintln!("fsync-probe needs --dir");
                2
            }
        },
        Some("core-bench") => bench::core_bench(num(&args, "--orders", 200_000)),
        Some("digest") => {
            println!("{}", digest::source_digest(&root()));
            0
        }
        Some("docs") => registry::run(&root()),
        Some("layers") => layers::run(&root()),
        Some("mutants") => mutants::run(&root(), opt(&args, "--only").as_deref()),
        Some("recovery-time") => perf::recovery_time(
            opt(&args, "--dir").unwrap_or_else(|| "data/perf".into()).into(),
            num(&args, "--orders", 18_000),
        ),
        Some("verify") => verify::run(
            &root(),
            args.iter().any(|a| a == "--quick"),
            args.iter().any(|a| a == "--live-testnet"),
            opt(&args, "--extended"),
        ),
        Some("perf") => {
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .unwrap_or_default();
            let label = opt(&args, "--label").unwrap_or_else(|| "perf".into());
            perf::run(perf::PerfCfg {
                dir: opt(&args, "--dir").unwrap_or_else(|| "data/perf".into()).into(),
                bin_dir: opt(&args, "--bin-dir").map(Into::into).unwrap_or(exe_dir),
                rates: opt(&args, "--rates")
                    .unwrap_or_else(|| "20,200,2000".into())
                    .split(',')
                    .filter_map(|r| r.parse().ok())
                    .collect(),
                seconds: num(&args, "--seconds", 10),
                connections: num(&args, "--connections", 4),
                inline: args.iter().any(|a| a == "--inline"),
                out: opt(&args, "--out").unwrap_or_else(|| format!("evidence/perf/{label}.json")).into(),
                label,
                extra_runtime: opt(&args, "--runtime").unwrap_or_default(),
                default_limits: args.iter().any(|a| a == "--default-limits"),
            })
        }
        _ => {
            eprintln!(
                "usage: xtask <command>\n\
                 \x20 verify [--quick] [--live-testnet] [--extended <dir on a real disk>]   acceptance suite -> evidence/latest/\n\
                 \x20 mutants [--only <id>]                 do the tests notice deliberately broken code -> evidence/mutants.json\n\
                 \x20 perf --label L [--dir D] [--seconds N] [--rates a,b,c] [--connections N] [--inline] [--default-limits]\n\
                 \x20 recovery-time [--dir D] [--orders N]\n\
                 \x20 fsync-probe --dir D [--label L] [--count N] [--bytes N]\n\
                 \x20 core-bench [--orders N]\n\
                 \x20 docs                                  registry + evidence -> docs/generated/, dashboard/data/\n\
                 \x20 layers                                crate layering table tools/ci/layers.toml (CI)\n\
                 \x20 digest                                source digest of this tree"
            );
            2
        }
    };
    std::process::exit(code);
}
