//! A small blocking client. One request at a time; events that arrive while
//! waiting for a response are queued.

use crate::*;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

#[derive(Debug)]
pub enum ClientError {
    Io(std::io::Error),
    Protocol(String),
    Api(ApiError),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Io(e) => write!(f, "io: {e}"),
            ClientError::Protocol(e) => write!(f, "protocol: {e}"),
            ClientError::Api(e) => write!(f, "{:?}: {}", e.code, e.message),
        }
    }
}
impl std::error::Error for ClientError {}

impl From<std::io::Error> for ClientError {
    fn from(e: std::io::Error) -> Self {
        ClientError::Io(e)
    }
}

pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
    pub events: VecDeque<EventMsg>,
}

impl Client {
    pub fn connect(path: &Path, strategy: Option<&str>) -> Result<Client, ClientError> {
        let stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        let writer = stream.try_clone()?;
        let mut c = Client {
            reader: BufReader::new(stream),
            writer,
            next_id: 1,
            events: VecDeque::new(),
        };
        let strategy = match strategy {
            Some(s) => Some(StrategyId::new(s).map_err(|e| ClientError::Protocol(e.to_string()))?),
            None => None,
        };
        c.call(Op::Hello {
            strategy,
            client: format!("oms-api/{}", env!("CARGO_PKG_VERSION")),
        })?;
        Ok(c)
    }

    pub fn set_timeout(&mut self, d: Duration) -> Result<(), ClientError> {
        self.reader.get_ref().set_read_timeout(Some(d))?;
        Ok(())
    }

    /// Sends a request without waiting. Returns its id.
    pub fn send(&mut self, op: Op) -> Result<u64, ClientError> {
        let id = self.next_id;
        self.next_id += 1;
        let mut line = serde_json::to_vec(&Request { id, op }).map_err(|e| ClientError::Protocol(e.to_string()))?;
        line.push(b'\n');
        self.writer.write_all(&line)?;
        Ok(id)
    }

    /// Reads the next message from the server.
    pub fn read_msg(&mut self) -> Result<ServerMsg, ClientError> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line)?;
        if n == 0 {
            return Err(ClientError::Protocol("connection closed by the server".into()));
        }
        serde_json::from_str(&line).map_err(|e| ClientError::Protocol(format!("{e}: {line}")))
    }

    /// Reads until the next response, queueing events.
    pub fn read_response(&mut self) -> Result<Response, ClientError> {
        loop {
            match self.read_msg()? {
                ServerMsg::Response(r) => return Ok(r),
                ServerMsg::Event(e) => self.events.push_back(e),
            }
        }
    }

    /// Sends one request and waits for its response.
    pub fn call(&mut self, op: Op) -> Result<serde_json::Value, ClientError> {
        let id = self.send(op)?;
        loop {
            let r = self.read_response()?;
            if r.id != id {
                continue;
            }
            return match (r.ok, r.error) {
                (true, _) => Ok(r.result.unwrap_or(serde_json::Value::Null)),
                (false, Some(e)) => Err(ClientError::Api(e)),
                (false, None) => Err(ClientError::Protocol("failure without an error".into())),
            };
        }
    }

    pub fn new_order(&mut self, op: Op) -> Result<NewOrderReply, ClientError> {
        let v = self.call(op)?;
        serde_json::from_value(v).map_err(|e| ClientError::Protocol(e.to_string()))
    }

    pub fn cancel(&mut self, order_id: OrderId) -> Result<CancelReply, ClientError> {
        let v = self.call(Op::Cancel {
            order_id: Some(order_id),
            client_order_id: None,
        })?;
        serde_json::from_value(v).map_err(|e| ClientError::Protocol(e.to_string()))
    }

    pub fn get_order(&mut self, order_id: OrderId) -> Result<Option<Order>, ClientError> {
        match self.call(Op::GetOrder {
            order_id: Some(order_id),
            client_order_id: None,
        }) {
            Ok(v) => serde_json::from_value(v).map(Some).map_err(|e| ClientError::Protocol(e.to_string())),
            Err(ClientError::Api(e)) if e.code == ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Waits for the next subscribed event, up to the read timeout.
    pub fn next_event(&mut self) -> Result<EventMsg, ClientError> {
        if let Some(e) = self.events.pop_front() {
            return Ok(e);
        }
        loop {
            if let ServerMsg::Event(e) = self.read_msg()? {
                return Ok(e);
            }
        }
    }
}

/// Microseconds since the Unix epoch on this host's clock.
pub fn now_us() -> Ts {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as Ts)
        .unwrap_or(0)
}
