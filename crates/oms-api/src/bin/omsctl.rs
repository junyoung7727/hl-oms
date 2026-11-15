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

use oms_api::client::{Client, ClientError, now_us};
use oms_api::*;
use oms_core::*;
use std::path::PathBuf;

fn opt(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn need(args: &[String], name: &str) -> Result<String, String> {
    opt(args, name).ok_or_else(|| format!("missing {name}"))
}

fn parse<T, E: std::fmt::Display>(what: &str, r: Result<T, E>) -> Result<T, String> {
    r.map_err(|e| format!("{what}: {e}"))
}

fn target_strategy(args: &[String]) -> Result<Option<StrategyId>, String> {
    opt(args, "--target-strategy")
        .map(|s| parse("--target-strategy", StrategyId::new(s)))
        .transpose()
}

fn order_ref(args: &[String]) -> Result<(Option<OrderId>, Option<ClientOrderId>), String> {
    let order = opt(args, "--order")
        .map(|s| parse("--order", s.parse::<u64>()).map(OrderId))
        .transpose()?;
    let key = opt(args, "--key")
        .map(|s| parse("--key", ClientOrderId::new(s)))
        .transpose()?;
    if order.is_none() && key.is_none() {
        return Err("name an order with --order or --key".into());
    }
    Ok((order, key))
}

fn build(cmd: &str, args: &[String]) -> Result<Op, String> {
    Ok(match cmd {
        "status" => Op::Status,
        "open" => Op::ListOpen,
        "snapshot" => Op::Snapshot,
        "new" => {
            let side = match need(args, "--side")?.as_str() {
                "buy" => Side::Buy,
                "sell" => Side::Sell,
                other => return Err(format!("--side: {other}")),
            };
            let tif = match opt(args, "--tif").as_deref().unwrap_or("gtc") {
                "gtc" => Tif::Gtc,
                "ioc" => Tif::Ioc,
                "alo" => Tif::Alo,
                other => return Err(format!("--tif: {other}")),
            };
            let ttl_ms: u64 = parse("--ttl-ms", opt(args, "--ttl-ms").unwrap_or_else(|| "5000".into()).parse())?;
            Op::NewOrder {
                client_order_id: parse("--key", ClientOrderId::new(need(args, "--key")?))?,
                instrument: parse("--instrument", InstrumentId::new(need(args, "--instrument")?))?,
                side,
                qty: parse("--qty", Qty::parse(&need(args, "--qty")?))?,
                px: parse("--px", Px::parse(&need(args, "--px")?))?,
                tif,
                reduce_only: args.iter().any(|a| a == "--reduce-only"),
                expires_at: now_us() + ttl_ms * 1000,
            }
        }
        "cancel" => {
            let (order_id, client_order_id) = order_ref(args)?;
            Op::Cancel { order_id, client_order_id }
        }
        "get" => {
            let (order_id, client_order_id) = order_ref(args)?;
            Op::GetOrder { order_id, client_order_id }
        }
        "halt" => Op::Halt { strategy: target_strategy(args)?, note: need(args, "--note")? },
        "resume" => Op::Resume { strategy: target_strategy(args)?, note: need(args, "--note")? },
        "cancel-all" => Op::CancelAll { strategy: target_strategy(args)?, note: need(args, "--note")? },
        "ack" => Op::AckMismatch { mismatch_id: parse("--id", need(args, "--id")?.parse())?, note: need(args, "--note")? },
        "resolve" => Op::ResolveOrder {
            order_id: OrderId(parse("--order", need(args, "--order")?.parse())?),
            note: need(args, "--note")?,
        },
        "adjust" => Op::AdjustPosition {
            instrument: parse("--instrument", InstrumentId::new(need(args, "--instrument")?))?,
            strategy: target_strategy(args)?,
            delta: parse("--delta", SQty::parse(&need(args, "--delta")?))?,
            note: need(args, "--note")?,
        },
        other => return Err(format!("unknown command {other}")),
    })
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let socket = PathBuf::from(need(&args, "--socket")?);
    let strategy = opt(&args, "--strategy");
    let flags_with_value = [
        "--socket", "--strategy", "--key", "--instrument", "--side", "--qty", "--px", "--tif", "--ttl-ms", "--order",
        "--note", "--id", "--delta", "--target-strategy",
    ];
    let mut cmd = None;
    let mut i = 0;
    while i < args.len() {
        if flags_with_value.contains(&args[i].as_str()) {
            i += 2;
        } else if args[i].starts_with("--") {
            i += 1;
        } else {
            cmd = Some(args[i].clone());
            break;
        }
    }
    let cmd = cmd.ok_or("no command given")?;
    let mut client = Client::connect(&socket, strategy.as_deref()).map_err(|e| e.to_string())?;
    if cmd == "watch" {
        client.call(Op::Subscribe).map_err(|e| e.to_string())?;
        client.set_timeout(std::time::Duration::from_secs(3600)).map_err(|e| e.to_string())?;
        loop {
            match client.next_event() {
                Ok(ev) => println!("{}", serde_json::to_string(&ev).unwrap_or_default()),
                Err(e) => return Err(e.to_string()),
            }
        }
    }
    let op = build(&cmd, &args)?;
    match client.call(op) {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            Ok(())
        }
        Err(ClientError::Api(e)) => Err(format!("{:?}: {}", e.code, e.message)),
        Err(e) => Err(e.to_string()),
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("omsctl: {e}");
        std::process::exit(1);
    }
}
