//! Where received frames and parsed messages go.

use md_core::{Normalized, StreamKey};

/// The record source (lake contract 2.2: `0` websocket, `1` rest).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Src {
    Ws,
    Rest,
}

/// A received frame, as the collector writes it to L0: the fields of an L0
/// record plus the plan slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFrame {
    /// CLOCK_REALTIME at receive, ns.
    pub recv_ns: i64,
    pub src: Src,
    /// `0` for REST bodies.
    pub conn_id: u32,
    /// Position of the frame in its connection, from 0. For REST bodies, the
    /// position among the session's REST bodies.
    pub frame_no: u64,
    /// `ConnPlan::conn_index` of the slot the frame belongs to.
    pub conn_index: u32,
    /// For REST bodies: the topic the snapshot was requested for.
    pub topic: Option<StreamKey>,
    /// For REST bodies: the connection whose book the snapshot starts.
    pub for_conn_id: Option<u32>,
    pub payload: Vec<u8>,
}

/// The sink cannot take the frame (its queue is full). The feed closes the
/// connection with reason `overflow` (lake contract 2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overflow;

/// Receives every frame. Called on the connection's task: it must not block
/// (the collector puts a bounded queue behind it).
pub trait FrameSink: Send + Sync {
    fn frame(&self, f: RawFrame) -> Result<(), Overflow>;

    /// The connection `conn_id` has ended: called once by its own task,
    /// after its last [`FrameSink::frame`] call. Default: nothing.
    fn conn_ended(&self, _conn_id: u32) {}
}

/// Receives the parsed trades and tickers of the active connections.
pub trait MessageSink: Send + Sync {
    fn message(&self, conn_id: u32, msg: &Normalized);
}
