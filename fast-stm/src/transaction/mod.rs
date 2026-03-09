#[cfg(feature = "wait-on-retry")]
pub mod control_block;
pub mod log_var;

cfg_if::cfg_if! {
    if #[cfg(feature = "hash-registers")] {
        use std::collections::hash_map::Entry;

        use rustc_hash::FxHashMap;
    } else {
        use std::collections::{btree_map::Entry, BTreeMap};
    }
}

use std::any::Any;
use std::cell::Cell;
use std::mem;
use std::sync::Arc;

use crate::result::{StmClosureResult, StmError};
use crate::tvar::{TVar, VarControlBlock};
use crate::{TransactionClosureResult, TransactionError, TransactionResult};

#[cfg(feature = "wait-on-retry")]
use control_block::ControlBlock;
use log_var::LogVar;

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

#[cfg(feature = "profiling")]
#[derive(Debug, Default)]
pub struct TransactionTallies {
    pub n_attempts: std::sync::atomic::AtomicUsize,
    pub n_retry: std::sync::atomic::AtomicUsize,
    pub n_error: std::sync::atomic::AtomicUsize,
    pub n_read: std::sync::atomic::AtomicUsize,
    pub n_redundant_read: std::sync::atomic::AtomicUsize,
    pub n_read_after_write: std::sync::atomic::AtomicUsize,
    pub n_write: std::sync::atomic::AtomicUsize,
}

#[cfg(feature = "profiling")]
impl std::ops::AddAssign for TransactionTallies {
    fn add_assign(&mut self, rhs: Self) {
        self.n_attempts.fetch_add(
            rhs.n_attempts.load(std::sync::atomic::Ordering::Relaxed),
            std::sync::atomic::Ordering::Relaxed,
        );
        self.n_retry.fetch_add(
            rhs.n_retry.load(std::sync::atomic::Ordering::Relaxed),
            std::sync::atomic::Ordering::Relaxed,
        );
        self.n_error.fetch_add(
            rhs.n_error.load(std::sync::atomic::Ordering::Relaxed),
            std::sync::atomic::Ordering::Relaxed,
        );
        self.n_read.fetch_add(
            rhs.n_read.load(std::sync::atomic::Ordering::Relaxed),
            std::sync::atomic::Ordering::Relaxed,
        );
        self.n_redundant_read.fetch_add(
            rhs.n_redundant_read
                .load(std::sync::atomic::Ordering::Relaxed),
            std::sync::atomic::Ordering::Relaxed,
        );
        self.n_read_after_write.fetch_add(
            rhs.n_read_after_write
                .load(std::sync::atomic::Ordering::Relaxed),
            std::sync::atomic::Ordering::Relaxed,
        );
        self.n_write.fetch_add(
            rhs.n_write.load(std::sync::atomic::Ordering::Relaxed),
            std::sync::atomic::Ordering::Relaxed,
        );
    }
}

#[cfg(feature = "profiling")]
impl std::iter::Sum for TransactionTallies {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::default(), |mut acc, t| {
            acc += t;
            acc
        })
    }
}

// -- Transactions

#[cfg(not(feature = "hash-registers"))]
pub(crate) type RegisterType = BTreeMap<Arc<VarControlBlock>, LogVar>;
#[cfg(feature = "hash-registers")]
pub(crate) type RegisterType = FxHashMap<*const VarControlBlock, LogVar>;

/// Transaction tracks all the read and written variables.
///
/// It is used for checking vars, to ensure atomicity.
pub struct Transaction {
    /// Map of all vars that map the `VarControlBlock` of a var to a `LogVar`.
    /// The `VarControlBlock` is unique because it uses it's address for comparing.
    ///
    /// The logs need to be accessed in a order to prevend dead-locks on locking.
    vars: RegisterType,
    #[cfg(feature = "profiling")]
    tallies: TransactionTallies,
}

impl Default for Transaction {
    fn default() -> Self {
        Self {
            vars: RegisterType::default(),
            #[cfg(feature = "profiling")]
            tallies: TransactionTallies::default(),
        }
    }
}

/// Public API
impl Transaction {
    /// Run a function with a transaction.
    ///
    /// It is equivalent to `atomically`.
    pub fn with<T, F>(f: F) -> T
    where
        F: Fn(&mut Transaction) -> StmClosureResult<T>,
    {
        match Transaction::with_control(|_| TransactionControl::Retry, f) {
            Some(t) => t,
            None => unreachable!(),
        }
    }

    /// Run a function with a transaction.
    ///
    /// `with_control` takes another control function, that
    /// can steer the control flow and possible terminate early.
    ///
    /// `control` can react to counters, timeouts or external inputs.
    ///
    /// It allows the user to fall back to another strategy, like a global lock
    /// in the case of too much contention.
    ///
    /// Please not, that the transaction may still infinitely wait for changes when `retry` is
    /// called and `control` does not abort.
    /// If you need a timeout, another thread should signal this through a [`TVar`].
    pub fn with_control<T, F, C>(mut control: C, f: F) -> Option<T>
    where
        F: Fn(&mut Transaction) -> StmClosureResult<T>,
        C: FnMut(StmError) -> TransactionControl,
    {
        let _guard = TransactionGuard::new();

        // create a log guard for initializing and cleaning up
        // the log
        let mut transaction = Transaction::default();

        // loop until success
        loop {
            // run the computation
            match f(&mut transaction) {
                // on success exit loop
                Ok(t) => {
                    if transaction.commit() {
                        return Some(t);
                    }
                }

                Err(e) => {
                    // Check if the user wants to abort the transaction.
                    if let TransactionControl::Abort = control(e) {
                        return None;
                    }

                    // on retry wait for changes
                    #[cfg(feature = "wait-on-retry")]
                    if let StmError::Retry = e {
                        transaction.wait_for_change();
                    }
                }
            }

            // clear log before retrying computation
            transaction.clear();
        }
    }

    /// Run a function with a transaction.
    ///
    /// The transaction will be retried until:
    /// - it is validated, or
    /// - it is explicitly aborted from the function, using the `TODO` function.
    pub fn with_err<T, F, E>(f: F) -> Result<T, E>
    where
        F: Fn(&mut Transaction) -> TransactionClosureResult<T, E>,
    {
        let _guard = TransactionGuard::new();

        // create a log guard for initializing and cleaning up
        // the log
        let mut transaction = Transaction::default();

        // loop until success
        loop {
            // run the computation
            match f(&mut transaction) {
                // on success exit loop
                Ok(t) => {
                    if transaction.commit() {
                        return Ok(t);
                    }
                }
                // on error,
                Err(e) => match e {
                    // abort and return the error
                    TransactionError::Abort(err) => return Err(err),
                    // retry
                    TransactionError::Stm(_) => {
                        #[cfg(feature = "wait-on-retry")]
                        transaction.wait_for_change();
                    }
                },
            }

            // clear log before retrying computation
            transaction.clear();
        }
    }

    /// Run a function with a transaction.
    ///
    /// `with_control` takes another control function, that
    /// can steer the control flow and possible terminate early.
    ///
    /// `control` can react to counters, timeouts or external inputs.
    ///
    /// It allows the user to fall back to another strategy, like a global lock
    /// in the case of too much contention.
    ///
    /// Please not, that the transaction may still infinitely wait for changes when `retry` is
    /// called and `control` does not abort.
    /// If you need a timeout, another thread should signal this through a [`TVar`].
    pub fn with_control_and_err<T, F, C, E>(mut control: C, f: F) -> TransactionResult<T, E>
    where
        F: Fn(&mut Transaction) -> TransactionClosureResult<T, E>,
        C: FnMut(StmError) -> TransactionControl,
    {
        let _guard = TransactionGuard::new();

        // create a log guard for initializing and cleaning up
        // the log
        let mut transaction = Transaction::default();

        // loop until success
        loop {
            // run the computation
            match f(&mut transaction) {
                // on success exit loop
                Ok(t) => {
                    if transaction.commit() {
                        return TransactionResult::Validated(t);
                    }
                }

                Err(e) => {
                    match e {
                        TransactionError::Abort(err) => {
                            return TransactionResult::Cancelled(err);
                        }
                        TransactionError::Stm(err) => {
                            // Check if the user wants to abort the transaction.
                            if let TransactionControl::Abort = control(err) {
                                return TransactionResult::Abandoned;
                            }

                            // on retry wait for changes
                            #[cfg(feature = "wait-on-retry")]
                            if let StmError::Retry = err {
                                transaction.wait_for_change();
                            }
                        }
                    }
                }
            }

            // clear log before retrying computation
            transaction.clear();
        }
    }
}

#[cfg(feature = "profiling")]
/// Public profiling API
impl Transaction {
    /// Run a function with a transaction.
    ///
    /// It is equivalent to `atomically`.
    pub fn profile_with<T, F>(f: F) -> (T, TransactionTallies)
    where
        F: Fn(&mut Transaction) -> StmClosureResult<T>,
    {
        match Transaction::profile_with_control(|_| TransactionControl::Retry, f) {
            (Some(t), tallies) => (t, tallies),
            (None, _) => unreachable!(),
        }
    }

    /// Run a function with a transaction.
    ///
    /// `with_control` takes another control function, that
    /// can steer the control flow and possible terminate early.
    ///
    /// `control` can react to counters, timeouts or external inputs.
    ///
    /// It allows the user to fall back to another strategy, like a global lock
    /// in the case of too much contention.
    ///
    /// Please not, that the transaction may still infinitely wait for changes when `retry` is
    /// called and `control` does not abort.
    /// If you need a timeout, another thread should signal this through a [`TVar`].
    pub fn profile_with_control<T, F, C>(mut control: C, f: F) -> (Option<T>, TransactionTallies)
    where
        F: Fn(&mut Transaction) -> StmClosureResult<T>,
        C: FnMut(StmError) -> TransactionControl,
    {
        let _guard = TransactionGuard::new();

        // create a log guard for initializing and cleaning up
        // the log
        let mut transaction = Transaction::default();

        // loop until success
        loop {
            transaction
                .tallies
                .n_attempts
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // run the computation
            match f(&mut transaction) {
                // on success exit loop
                Ok(t) => {
                    if transaction.commit() {
                        return (Some(t), transaction.tallies);
                    }
                }

                Err(e) => {
                    // Check if the user wants to abort the transaction.
                    match e {
                        StmError::Failure => {
                            transaction
                                .tallies
                                .n_error
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        StmError::Retry => {
                            transaction
                                .tallies
                                .n_retry
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }

                    if let TransactionControl::Abort = control(e) {
                        return (None, transaction.tallies);
                    }

                    // on retry wait for changes
                    #[cfg(feature = "wait-on-retry")]
                    if let StmError::Retry = e {
                        transaction.wait_for_change();
                    }
                }
            }

            // clear log before retrying computation
            transaction.clear();
        }
    }

    /// Run a function with a transaction.
    ///
    /// The transaction will be retried until:
    /// - it is validated, or
    /// - it is explicitly aborted from the function, using the `TODO` function.
    pub fn profile_with_err<T, F, E>(f: F) -> (Result<T, E>, TransactionTallies)
    where
        F: Fn(&mut Transaction) -> TransactionClosureResult<T, E>,
    {
        let _guard = TransactionGuard::new();

        // create a log guard for initializing and cleaning up
        // the log
        let mut transaction = Transaction::default();

        // loop until success
        loop {
            transaction
                .tallies
                .n_attempts
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // run the computation
            match f(&mut transaction) {
                // on success exit loop
                Ok(t) => {
                    if transaction.commit() {
                        return (Ok(t), transaction.tallies);
                    }
                }
                // on error,
                Err(e) => match e {
                    // abort and return the error
                    TransactionError::Abort(err) => return (Err(err), transaction.tallies),
                    // retry
                    TransactionError::Stm(err) => {
                        match err {
                            StmError::Failure => {
                                transaction
                                    .tallies
                                    .n_error
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                            StmError::Retry => {
                                transaction
                                    .tallies
                                    .n_retry
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                        #[cfg(feature = "wait-on-retry")]
                        transaction.wait_for_change();
                    }
                },
            }

            // clear log before retrying computation
            transaction.clear();
        }
    }

    /// Run a function with a transaction.
    ///
    /// `with_control` takes another control function, that
    /// can steer the control flow and possible terminate early.
    ///
    /// `control` can react to counters, timeouts or external inputs.
    ///
    /// It allows the user to fall back to another strategy, like a global lock
    /// in the case of too much contention.
    ///
    /// Please not, that the transaction may still infinitely wait for changes when `retry` is
    /// called and `control` does not abort.
    /// If you need a timeout, another thread should signal this through a [`TVar`].
    pub fn profile_with_control_and_err<T, F, C, E>(
        mut control: C,
        f: F,
    ) -> (TransactionResult<T, E>, TransactionTallies)
    where
        F: Fn(&mut Transaction) -> TransactionClosureResult<T, E>,
        C: FnMut(StmError) -> TransactionControl,
    {
        let _guard = TransactionGuard::new();

        // create a log guard for initializing and cleaning up
        // the log
        let mut transaction = Transaction::default();

        // loop until success
        loop {
            transaction
                .tallies
                .n_attempts
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // run the computation
            match f(&mut transaction) {
                // on success exit loop
                Ok(t) => {
                    if transaction.commit() {
                        return (TransactionResult::Validated(t), transaction.tallies);
                    }
                }

                Err(e) => {
                    match e {
                        TransactionError::Abort(err) => {
                            return (TransactionResult::Cancelled(err), transaction.tallies);
                        }
                        TransactionError::Stm(err) => {
                            match err {
                                StmError::Failure => {
                                    transaction
                                        .tallies
                                        .n_error
                                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                }
                                StmError::Retry => {
                                    transaction
                                        .tallies
                                        .n_retry
                                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                }
                            }

                            // Check if the user wants to abort the transaction.
                            if let TransactionControl::Abort = control(err) {
                                return (TransactionResult::Abandoned, transaction.tallies);
                            }

                            // on retry wait for changes
                            #[cfg(feature = "wait-on-retry")]
                            if let StmError::Retry = err {
                                transaction.wait_for_change();
                            }
                        }
                    }
                }
            }

            // clear log before retrying computation
            transaction.clear();
        }
    }
}

/// In-closure routines
impl Transaction {
    /// Read a variable and return the value.
    ///
    /// The returned value is not always consistent with the current value of the var,
    /// but may be an outdated or or not yet commited value.
    ///
    /// The used code should be capable of handling inconsistent states
    /// without running into infinite loops.
    /// Just the commit of wrong values is prevented by STM.
    pub fn read<T: Send + Sync + Any + Clone>(&mut self, var: &TVar<T>) -> StmClosureResult<T> {
        #[cfg(feature = "profiling")]
        self.tallies
            .n_read
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let ctrl = var.control_block().clone();
        // Check if the same var was written before.
        #[cfg(not(feature = "hash-registers"))]
        let key = ctrl;
        #[cfg(feature = "hash-registers")]
        let key = Arc::as_ptr(&ctrl);
        let value = match self.vars.entry(key) {
            // If the variable has been accessed before, then load that value.
            #[cfg(feature = "early-conflict-detection")]
            Entry::Occupied(mut entry) => {
                let log = entry.get_mut();
                // if we previously read the var, check for value change
                if let LogVar::Read(v) = log {
                    #[cfg(feature = "profiling")]
                    self.tallies
                        .n_redundant_read
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let crt_v = var.read_ref_atomic();
                    if !Arc::ptr_eq(v, &crt_v) {
                        return Err(StmError::Failure);
                    }
                }
                #[cfg(feature = "profiling")]
                if let LogVar::Write(_) = log {
                    self.tallies
                        .n_read_after_write
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                log.read()
            }
            #[cfg(not(feature = "early-conflict-detection"))]
            Entry::Occupied(mut entry) => {
                #[cfg(feature = "profiling")]
                {
                    let log = entry.get();
                    if let LogVar::Read(_) = log {
                        self.tallies
                            .n_redundant_read
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    } else if let LogVar::Write(_) = log {
                        self.tallies
                            .n_read_after_write
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }

                entry.get_mut().read()
            }

            // Else load the variable statically.
            Entry::Vacant(entry) => {
                // Read the value from the var.
                let value = var.read_ref_atomic();

                // Store in in an entry.
                entry.insert(LogVar::Read(value.clone()));
                value
            }
        };

        Ok(Transaction::downcast(value))
    }

    /// Write a variable.
    ///
    /// The write is not immediately visible to other threads,
    /// but atomically commited at the end of the computation.
    pub fn write<T: Any + Send + Sync + Clone>(
        &mut self,
        var: &TVar<T>,
        value: T,
    ) -> StmClosureResult<()> {
        #[cfg(feature = "profiling")]
        self.tallies
            .n_write
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // box the value
        let boxed = Arc::new(value);

        // new control block
        let ctrl = var.control_block().clone();
        // update or create new entry
        #[cfg(not(feature = "hash-registers"))]
        let key = ctrl;
        #[cfg(feature = "hash-registers")]
        let key = Arc::as_ptr(&ctrl);
        match self.vars.entry(key) {
            Entry::Occupied(mut entry) => entry.get_mut().write(boxed),
            Entry::Vacant(entry) => {
                entry.insert(LogVar::Write(boxed));
            }
        }

        // For now always succeeds, but that may change later.
        Ok(())
    }

    /// Modify a variable.
    ///
    /// The write is not immediately visible to other threads,
    /// but atomically commited at the end of the computation.
    ///
    /// Prefer this method over calling `read` then `write` for performance.
    pub fn modify<T: Any + Send + Sync + Clone, F>(
        &mut self,
        var: &TVar<T>,
        f: F,
    ) -> StmClosureResult<()>
    where
        F: FnOnce(T) -> T,
    {
        #[cfg(feature = "profiling")]
        self.tallies
            .n_write
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // new control block
        let ctrl = var.control_block().clone();
        // update or create new entry
        #[cfg(not(feature = "hash-registers"))]
        let key = ctrl;
        #[cfg(feature = "hash-registers")]
        let key = Arc::as_ptr(&ctrl);
        match self.vars.entry(key) {
            // If the variable has been accessed before, then load that value.
            #[cfg(feature = "early-conflict-detection")]
            Entry::Occupied(mut entry) => {
                let log = entry.get_mut();
                // if we previously read the var, check for value change
                if let LogVar::Read(v) = log {
                    #[cfg(feature = "profiling")]
                    self.tallies
                        .n_redundant_read
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let crt_v = var.read_ref_atomic();
                    if !Arc::ptr_eq(v, &crt_v) {
                        return Err(StmError::Failure);
                    }
                }
                #[cfg(feature = "profiling")]
                if let LogVar::Write(_) = log {
                    self.tallies
                        .n_read_after_write
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                let value = Transaction::downcast(log.read());
                let boxed = Arc::new(f(value));
                entry.get_mut().write(boxed);
            }
            #[cfg(not(feature = "early-conflict-detection"))]
            Entry::Occupied(mut entry) => {
                #[cfg(feature = "profiling")]
                {
                    let log = entry.get();
                    if let LogVar::Read(_) = log {
                        self.tallies
                            .n_redundant_read
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    } else if let LogVar::Write(_) = log {
                        self.tallies
                            .n_read_after_write
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }

                let value = Transaction::downcast(entry.get_mut().read());
                let boxed = Arc::new(f(value));
                entry.get_mut().write(boxed);
            }
            Entry::Vacant(entry) => {
                // Read the value from the var.
                let value = Transaction::downcast(var.read_ref_atomic());
                let boxed = Arc::new(f(value));
                entry.insert(LogVar::Write(boxed));
            }
        }

        // For now always succeeds, but that may change later.
        Ok(())
    }

    /// Replace a variable.
    ///
    /// The write is not immediately visible to other threads,
    /// but atomically commited at the end of the computation.
    ///
    /// Prefer this method over calling `read` then `write` for performance.
    pub fn replace<T: Any + Send + Sync + Clone>(
        &mut self,
        var: &TVar<T>,
        value: T,
    ) -> StmClosureResult<T> {
        #[cfg(feature = "profiling")]
        self.tallies
            .n_write
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // box the value
        let boxed = Arc::new(value);

        // new control block
        let ctrl = var.control_block().clone();
        // update or create new entry
        #[cfg(not(feature = "hash-registers"))]
        let key = ctrl;
        #[cfg(feature = "hash-registers")]
        let key = Arc::as_ptr(&ctrl);
        let value = match self.vars.entry(key) {
            // If the variable has been accessed before, then load that value.
            #[cfg(feature = "early-conflict-detection")]
            Entry::Occupied(mut entry) => {
                let log = entry.get_mut();
                // if we previously read the var, check for value change
                if let LogVar::Read(v) = log {
                    #[cfg(feature = "profiling")]
                    self.tallies
                        .n_redundant_read
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let crt_v = var.read_ref_atomic();
                    if !Arc::ptr_eq(v, &crt_v) {
                        return Err(StmError::Failure);
                    }
                }
                #[cfg(feature = "profiling")]
                if let LogVar::Write(_) = log {
                    self.tallies
                        .n_read_after_write
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                let value = log.read();
                entry.get_mut().write(boxed);
                value
            }
            #[cfg(not(feature = "early-conflict-detection"))]
            Entry::Occupied(mut entry) => {
                #[cfg(feature = "profiling")]
                {
                    let log = entry.get();
                    if let LogVar::Read(_) = log {
                        self.tallies
                            .n_redundant_read
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    } else if let LogVar::Write(_) = log {
                        self.tallies
                            .n_read_after_write
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }

                let value = entry.get_mut().read();
                entry.get_mut().write(boxed);
                value
            }
            Entry::Vacant(entry) => {
                // Read the value from the var.
                let value = var.read_ref_atomic();
                entry.insert(LogVar::Write(boxed));
                value
            }
        };

        // For now always succeeds, but that may change later.
        Ok(Transaction::downcast(value))
    }

    /// Combine two calculations. When one blocks with `retry`,
    /// run the other, but don't commit the changes in the first.
    ///
    /// If both block, `Transaction::or` still waits for `TVar`s in both functions.
    /// Use `Transaction::or` instead of handling errors directly with the `Result::or`.
    /// The later does not handle all the blocking correctly.
    pub fn or<T, F1, F2>(&mut self, first: F1, second: F2) -> StmClosureResult<T>
    where
        F1: Fn(&mut Transaction) -> StmClosureResult<T>,
        F2: Fn(&mut Transaction) -> StmClosureResult<T>,
    {
        // Create a backup of the log.
        let mut copy = self.vars.clone();
        // let mut copy = Transaction {
        //     vars: self.vars.clone(),
        // };

        // Run the first computation.
        let f = first(self);

        match f {
            // Run other on manual retry call.
            Err(StmError::Retry) => {
                // swap, so that self is the current run
                mem::swap(&mut self.vars, &mut copy);

                // Run other action.
                let s = second(self);

                // If both called retry then exit.
                match s {
                    Err(StmError::Failure) => Err(StmError::Failure),
                    s => {
                        self.combine(copy);
                        s
                    }
                }
            }

            // Return success and failure directly
            x => x,
        }
    }
}

/// Internal routines
impl Transaction {
    #[allow(clippy::needless_pass_by_value)]
    /// Perform a downcast on a var.
    fn downcast<T: Any + Clone>(var: Arc<dyn Any>) -> T {
        match var.downcast_ref::<T>() {
            Some(s) => s.clone(),
            None => unreachable!("TVar has wrong type"),
        }
    }

    /// Combine two logs into a single log, to allow waiting for all reads.
    fn combine(&mut self, vars: RegisterType) {
        // combine reads
        for (var, value) in vars {
            // only insert new values
            if let Some(value) = value.obsolete() {
                self.vars.entry(var).or_insert(value);
            }
        }
    }

    /// Clear the log's data.
    ///
    /// This should be used before redoing a computation, but
    /// nowhere else.
    fn clear(&mut self) {
        self.vars.clear();
    }

    /// Wait for any variable to change,
    /// because the change may lead to a new calculation result.
    #[cfg(feature = "wait-on-retry")]
    fn wait_for_change(&mut self) {
        // Create control block for waiting.
        let ctrl = Arc::new(ControlBlock::new());

        #[allow(clippy::mutable_key_type)]
        let vars = std::mem::take(&mut self.vars);
        let mut reads = Vec::with_capacity(self.vars.len());

        let blocking = vars
            .into_iter()
            .filter_map(|(a, b)| b.into_read_value().map(|b| (a, b)))
            // Check for consistency.
            .all(|(var, value)| {
                #[cfg(feature = "hash-registers")]
                let var = unsafe { var.as_ref() }.expect("E: unreachabel");
                var.wait(&ctrl);
                let x = {
                    // Take read lock and read value.
                    let guard = var.value.read();
                    Arc::ptr_eq(&value, &guard)
                };
                reads.push(var);
                x
            });

        // If no var has changed, then block.
        if blocking {
            // Propably wait until one var has changed.
            ctrl.wait();
        }

        // Let others know that ctrl is dead.
        // It does not matter, if we set too many
        // to dead since it may slightly reduce performance
        // but not break the semantics.
        for var in &reads {
            var.set_dead();
        }
    }

    /// Write the log back to the variables.
    ///
    /// Return true for success and false, if a read var has changed
    pub(crate) fn commit(&mut self) -> bool {
        // Use two phase locking for safely writing data back to the vars.

        // First phase: acquire locks.
        // Check for consistency of all the reads and perform
        // an early return if something is not consistent.

        // Created arrays for storing the locks
        // vector of locks.
        let mut read_vec = Vec::with_capacity(self.vars.len());

        // vector of tuple (value, lock)
        let mut write_vec = Vec::with_capacity(self.vars.len());

        // vector of written variables
        let mut written = Vec::with_capacity(self.vars.len());

        #[cfg(feature = "hash-registers")]
        let records = {
            let mut recs: Vec<_> = self.vars.iter().collect();
            recs.sort_by(|(k1, _), (k2, _)| k1.cmp(&k2));
            recs
        };
        #[cfg(not(feature = "hash-registers"))]
        let records = &self.vars;

        for (var, value) in records {
            // lock the variable and read the value
            #[cfg(feature = "hash-registers")]
            let var = unsafe { var.as_ref() }.expect("E: unreachabel");

            match *value {
                // We need to take a write lock.
                LogVar::Write(ref w) | LogVar::ReadObsoleteWrite(_, ref w) => {
                    // take write lock
                    let lock = var.value.write();
                    // add all data to the vector
                    write_vec.push((w, lock));
                    written.push(var);
                }

                // We need to check for consistency and
                // take a write lock.
                LogVar::ReadWrite(ref original, ref w) => {
                    // take write lock
                    let lock = var.value.write();

                    if !Arc::ptr_eq(&lock, original) {
                        return false;
                    }
                    // add all data to the vector
                    write_vec.push((w, lock));
                    written.push(var);
                }
                // Nothing to do. ReadObsolete is only needed for blocking, not
                // for consistency checks.
                LogVar::ReadObsolete(_) => {}
                // Take read lock and check for consistency.
                LogVar::Read(ref original) => {
                    // Take a read lock.
                    let lock = var.value.read();

                    if !Arc::ptr_eq(&lock, original) {
                        return false;
                    }

                    read_vec.push(lock);
                }
            }
        }

        // Second phase: write back and release

        // Release the reads first.
        // This allows other threads to continue quickly.
        drop(read_vec);

        for (value, mut lock) in write_vec {
            // Commit value.
            *lock = value.clone();
        }

        #[cfg(feature = "wait-on-retry")]
        for var in written {
            // Unblock all threads waiting for it.
            var.wake_all();
        }

        // Commit succeded.
        true
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn read() {
        let mut log = Transaction::default();
        let var = TVar::new(vec![1, 2, 3, 4]);

        // The variable can be read.
        assert_eq!(&*log.read(&var).unwrap(), &[1, 2, 3, 4]);
    }

    #[test]
    fn write_read() {
        let mut log = Transaction::default();
        let var = TVar::new(vec![1, 2]);

        log.write(&var, vec![1, 2, 3, 4]).unwrap();

        // Consecutive reads get the updated version.
        assert_eq!(log.read(&var).unwrap(), [1, 2, 3, 4]);

        // The original value is still preserved.
        assert_eq!(var.read_atomic(), [1, 2]);
    }

    #[test]
    fn transaction_simple() {
        let x = Transaction::with(|_| Ok(42));
        assert_eq!(x, 42);
    }

    #[test]
    fn transaction_read() {
        let read = TVar::new(42);

        let x = Transaction::with(|trans| read.read(trans));

        assert_eq!(x, 42);
    }

    /// Run a transaction with a control function, that always aborts.
    /// The transaction still tries to run a single time and should successfully
    /// commit in this test.
    #[test]
    fn transaction_with_control_abort_on_single_run() {
        let read = TVar::new(42);

        let x = Transaction::with_control(|_| TransactionControl::Abort, |tx| read.read(tx));

        assert_eq!(x, Some(42));
    }

    /// Run a transaction with a control function, that always aborts.
    /// The transaction retries infinitely often. The control function will abort this loop.
    #[test]
    fn transaction_with_control_abort_on_retry() {
        let x: Option<i32> =
            Transaction::with_control(|_| TransactionControl::Abort, |_| Err(StmError::Retry));

        assert_eq!(x, None);
    }

    #[test]
    fn transaction_write() {
        let write = TVar::new(42);

        Transaction::with(|trans| write.write(trans, 0));

        assert_eq!(write.read_atomic(), 0);
    }

    #[test]
    fn transaction_copy() {
        let read = TVar::new(42);
        let write = TVar::new(0);

        Transaction::with(|trans| {
            let r = read.read(trans)?;
            write.write(trans, r)
        });

        assert_eq!(write.read_atomic(), 42);
    }

    // Dat name. seriously?
    #[test]
    fn transaction_control_stuff() {
        let read = TVar::new(42);
        let write = TVar::new(0);

        Transaction::with(|trans| {
            let r = read.read(trans)?;
            write.write(trans, r)
        });

        assert_eq!(write.read_atomic(), 42);
    }

    /// Test if nested transactions are correctly detected.
    #[test]
    #[should_panic]
    fn transaction_nested_fail() {
        Transaction::with(|_| {
            Transaction::with(|_| Ok(42));
            Ok(1)
        });
    }
}
