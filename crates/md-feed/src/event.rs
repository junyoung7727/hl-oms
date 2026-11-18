//! What the feed reports. The collector turns these into L0 notes and metrics.

use md_core::adapter::Resync;
use md_core::{GapEvent, StreamKey};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedEvent {
    Connected {
        conn_index: u32,
        conn_id: u32,
    },
    /// A connection ended, or a connect failed (`conn_id` is then the number
    /// the attempt had).
    Disconnected {
        conn_index: u32,
        conn_id: u32,
        reason: String,
    },
    /// A delta broke the sequence rule; `resync` is what the feed does.
    Gap {
        conn_id: u32,
        key: StreamKey,
        gap: GapEvent,
        resync: Resync,
    },
    /// A snapshot arrived on a (connection, topic) that already had a book.
    Resnapshot {
        conn_id: u32,
        key: StreamKey,
        recv_ns: i64,
    },
    /// A REST resync applied its snapshot and replayed the buffered deltas.
    Resynced {
        conn_id: u32,
        key: StreamKey,
        recv_ns: i64,
    },
    ParseError {
        conn_id: u32,
        key: Option<StreamKey>,
        frame_no: u64,
        reason: String,
    },
    StaleRaised {
        conn_id: u32,
        key: StreamKey,
        /// `recv_ns` of the last applied message, if any.
        last_recv_ns: Option<i64>,
        now_ns: i64,
    },
    StaleCleared {
        conn_id: u32,
        key: StreamKey,
        now_ns: i64,
    },
    SwitchDone {
        conn_index: u32,
        from_conn_id: u32,
        to_conn_id: u32,
    },
    SwitchFailed {
        conn_index: u32,
        conn_id: u32,
        reason: String,
    },
}
