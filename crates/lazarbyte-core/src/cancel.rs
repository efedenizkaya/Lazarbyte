//! Cooperative cancellation for long scans.
//!
//! A phone user can leave the app while a scan is running, and the front end
//! has to be able to stop it without killing the process: whatever was already
//! recovered must stay on disk, with a manifest that says the run did not
//! finish. The flag is checked between chunks and between files, so a cancelled
//! scan stops within a bounded amount of work.
//!
//! Cancelling never deletes anything: it only stops the scan from starting new
//! work.

use std::sync::atomic::{AtomicBool, Ordering};

/// A flag one thread sets and the scanning threads read.
#[derive(Debug, Default)]
pub struct Cancel {
    cancelled: AtomicBool,
}

impl Cancel {
    pub fn new() -> Self {
        Cancel::default()
    }

    /// Ask the scan to stop. Safe to call from any thread, any number of times.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Has the scan been asked to stop? Callers poll this; it is never reset.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

/// True when a caller-supplied handle says to stop. `None` means the scan was
/// not offered a handle and runs to completion.
pub(crate) fn requested(cancel: Option<&std::sync::Arc<Cancel>>) -> bool {
    cancel.is_some_and(|c| c.is_cancelled())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_flag_is_not_cancelled() {
        let c = Cancel::new();
        assert!(!c.is_cancelled());
        assert!(!requested(None));
    }

    #[test]
    fn cancelling_is_sticky_and_visible_through_an_arc() {
        let c = std::sync::Arc::new(Cancel::new());
        c.cancel();
        c.cancel(); // idempotent
        assert!(c.is_cancelled());
        assert!(requested(Some(&std::sync::Arc::clone(&c))));
    }
}
