#[cfg(feature = "wait-on-retry")]
pub mod control_block;
pub mod log_var;
#[cfg(feature = "profiling")]
pub mod profiled;
#[cfg(not(feature = "profiling"))]
pub mod regular;

cfg_if::cfg_if! {
    if #[cfg(feature = "profiling")] {
        pub use profiled::{Transaction, TransactionTallies, atomically, atomically_with_err};
    } else {
        pub use regular::{Transaction, atomically, atomically_with_err};
    }
}

use std::cell::Cell;
cfg_if::cfg_if! {
    if #[cfg(feature = "hash-registers")] {
        use std::collections::hash_map::Entry;
        use rustc_hash::FxHashMap;
    } else {
        use std::{collections::BTreeMap, sync::Arc};
    }
}

use crate::tvar::VarControlBlock;
use log_var::LogVar;

#[cfg(not(feature = "hash-registers"))]
pub(crate) type RegisterType = BTreeMap<Arc<VarControlBlock>, LogVar>;
#[cfg(feature = "hash-registers")]
pub(crate) type RegisterType = FxHashMap<*const VarControlBlock, LogVar>;

thread_local!(static TRANSACTION_RUNNING: Cell<bool> = const { Cell::new(false) });

/// `TransactionGuard` checks against nested STM calls.
///
/// Use guard, so that it correctly marks the Transaction as finished.
struct TransactionGuard;

impl TransactionGuard {
    pub fn new() -> TransactionGuard {
        TRANSACTION_RUNNING.with(|t| {
            assert!(!t.get(), "STM: Nested Transaction");
            t.set(true);
        });
        TransactionGuard
    }
}

impl Drop for TransactionGuard {
    fn drop(&mut self) {
        TRANSACTION_RUNNING.with(|t| {
            t.set(false);
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionControl {
    Retry,
    Abort,
}
