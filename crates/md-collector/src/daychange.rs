//! The UTC day change (spec D7): every slot switches to a new overlapping
//! connection, staggered.

use std::sync::Arc;

use crate::collector::Inner;

pub(crate) async fn run(_inner: Arc<Inner>) {
    std::future::pending::<()>().await;
}
