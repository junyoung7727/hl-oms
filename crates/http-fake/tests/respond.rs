//! `FakeHttp::respond`: a handler that computes an answer from the request.

#![allow(clippy::unwrap_used)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use http_fake::{Answer, FakeHttp};

/// One request on a new connection; the status and the body of the answer
/// (read up to its `content-length`: the fake keeps the connection open).
fn get(fake: &FakeHttp, target: &str) -> (u16, String) {
    let mut s = TcpStream::connect(fake.addr()).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        s,
        "GET {target} HTTP/1.1\r\nhost: x\r\ncontent-length: 0\r\n\r\n"
    )
    .unwrap();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let text = String::from_utf8_lossy(&buf).into_owned();
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let len: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length: "))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            if body.len() >= len {
                let status = head
                    .split(' ')
                    .nth(1)
                    .and_then(|c| c.parse().ok())
                    .unwrap_or(0);
                return (status, body[..len].to_string());
            }
        }
        let n = s.read(&mut chunk).unwrap();
        assert!(n > 0, "the fake closed the connection: {text}");
        buf.extend_from_slice(&chunk[..n]);
    }
}

#[test]
fn a_handler_answer_wins_over_routes() {
    let fake = FakeHttp::start(Answer::Reply(200, "default".into()));
    fake.route("/a", [Answer::Reply(200, "route".into())]);
    fake.script([Answer::Reply(200, "script".into())]);
    fake.respond(|r| {
        (r.path() == "/a").then(|| Answer::Reply(201, format!("handler {}", r.query())))
    });
    assert_eq!(get(&fake, "/a?x=1"), (201, "handler x=1".to_string()));
    assert_eq!(get(&fake, "/a?x=2"), (201, "handler x=2".to_string()));
}

#[test]
fn a_handler_none_falls_through_to_routes_script_and_default() {
    let fake = FakeHttp::start(Answer::Reply(200, "default".into()));
    fake.route("/r", [Answer::Reply(202, "route".into())]);
    fake.script([Answer::Reply(203, "script".into())]);
    fake.respond(|r| (r.path() == "/h").then(|| Answer::Reply(201, "handler".into())));
    assert_eq!(get(&fake, "/r"), (202, "route".to_string()));
    assert_eq!(get(&fake, "/s"), (203, "script".to_string()));
    assert_eq!(get(&fake, "/s"), (200, "default".to_string()));
    assert_eq!(get(&fake, "/h"), (201, "handler".to_string()));
    assert_eq!(fake.requests().len(), 4);
}

/// The request is in the log before the handler runs, and the handler runs
/// without the fake's lock: it can read `requests()` (no deadlock).
#[test]
fn a_handler_sees_its_request_in_the_log() {
    let fake = Arc::new(FakeHttp::start(Answer::Reply(200, "default".into())));
    let weak = Arc::downgrade(&fake);
    fake.respond(move |r| {
        let f = weak.upgrade()?;
        let log = f.requests();
        let last = log.last().map(|l| l.target.clone()).unwrap_or_default();
        Some(Answer::Reply(
            200,
            format!("{} {last} {}", log.len(), r.target),
        ))
    });
    assert_eq!(get(&fake, "/one"), (200, "1 /one /one".to_string()));
    assert_eq!(get(&fake, "/two?q"), (200, "2 /two?q /two?q".to_string()));
}
