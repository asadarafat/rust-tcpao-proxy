use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct Metrics {
    open_connections: AtomicU64,
    closed_connections: AtomicU64,
}

impl Metrics {
    pub fn conn_opened(&self) {
        self.open_connections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn conn_closed(&self) {
        self.open_connections.fetch_sub(1, Ordering::Relaxed);
        self.closed_connections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn open_connections(&self) -> u64 {
        self.open_connections.load(Ordering::Relaxed)
    }

    pub fn closed_connections(&self) -> u64 {
        self.closed_connections.load(Ordering::Relaxed)
    }
}
