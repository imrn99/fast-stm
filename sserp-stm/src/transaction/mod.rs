#[cfg(feature = "wait-on-retry")]
pub mod control_block;

use std::any::Any;
use std::cell::Cell;
use std::collections::{btree_map::Entry, BTreeMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::result::{StmClosureResult, StmError};
use crate::tvar::{ArcAny, TVar, VarControlBlock};
use crate::{TransactionClosureResult, TransactionError, TransactionResult};

#[cfg(feature = "wait-on-retry")]
use control_block::ControlBlock;

static NEXT_TRANSACTION_ID: AtomicU64 = AtomicU64::new(1);

thread_local!(static TRANSACTION_RUNNING: Cell<bool> = const { Cell::new(false) });

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
        self.n_attempts
            .fetch_add(rhs.n_attempts.load(Ordering::Relaxed), Ordering::Relaxed);
        self.n_retry
            .fetch_add(rhs.n_retry.load(Ordering::Relaxed), Ordering::Relaxed);
        self.n_error
            .fetch_add(rhs.n_error.load(Ordering::Relaxed), Ordering::Relaxed);
        self.n_read
            .fetch_add(rhs.n_read.load(Ordering::Relaxed), Ordering::Relaxed);
        self.n_redundant_read.fetch_add(
            rhs.n_redundant_read.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.n_read_after_write.fetch_add(
            rhs.n_read_after_write.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.n_write
            .fetch_add(rhs.n_write.load(Ordering::Relaxed), Ordering::Relaxed);
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

#[derive(Clone)]
struct ReadEntry {
    timestamp: u64,
    obsolete: bool,
}

#[derive(Clone)]
struct WriteEntry {
    value: ArcAny,
}

type ReadSet = BTreeMap<Arc<VarControlBlock>, ReadEntry>;
type WriteSet = BTreeMap<Arc<VarControlBlock>, WriteEntry>;

pub struct Transaction {
    id: u64,
    clock: u64,
    reads: ReadSet,
    writes: WriteSet,
    #[cfg(feature = "profiling")]
    tallies: TransactionTallies,
}

impl Default for Transaction {
    fn default() -> Self {
        Self {
            id: NEXT_TRANSACTION_ID.fetch_add(1, Ordering::Relaxed),
            clock: 0,
            reads: ReadSet::default(),
            writes: WriteSet::default(),
            #[cfg(feature = "profiling")]
            tallies: TransactionTallies::default(),
        }
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        self.release_writes();
    }
}

impl Transaction {
    pub fn with<T, F>(f: F) -> T
    where
        F: Fn(&mut Transaction) -> StmClosureResult<T>,
    {
        match Transaction::with_control(|_| TransactionControl::Retry, f) {
            Some(t) => t,
            None => unreachable!(),
        }
    }

    pub fn with_control<T, F, C>(mut control: C, f: F) -> Option<T>
    where
        F: Fn(&mut Transaction) -> StmClosureResult<T>,
        C: FnMut(StmError) -> TransactionControl,
    {
        let _guard = TransactionGuard::new();
        let mut transaction = Transaction::default();

        loop {
            match f(&mut transaction) {
                Ok(t) => {
                    if transaction.commit() {
                        return Some(t);
                    }
                }
                Err(e) => {
                    if let TransactionControl::Abort = control(e) {
                        transaction.clear();
                        return None;
                    }

                    #[cfg(feature = "wait-on-retry")]
                    if let StmError::Retry = e {
                        transaction.wait_for_change();
                    }
                }
            }

            transaction.clear();
        }
    }

    pub fn with_err<T, F, E>(f: F) -> Result<T, E>
    where
        F: Fn(&mut Transaction) -> TransactionClosureResult<T, E>,
    {
        let _guard = TransactionGuard::new();
        let mut transaction = Transaction::default();

        loop {
            match f(&mut transaction) {
                Ok(t) => {
                    if transaction.commit() {
                        return Ok(t);
                    }
                }
                Err(e) => match e {
                    TransactionError::Abort(err) => {
                        transaction.clear();
                        return Err(err);
                    }
                    TransactionError::Stm(err) =>
                    {
                        #[cfg(feature = "wait-on-retry")]
                        if let StmError::Retry = err {
                            transaction.wait_for_change();
                        }
                    }
                },
            }

            transaction.clear();
        }
    }

    pub fn with_control_and_err<T, F, C, E>(mut control: C, f: F) -> TransactionResult<T, E>
    where
        F: Fn(&mut Transaction) -> TransactionClosureResult<T, E>,
        C: FnMut(StmError) -> TransactionControl,
    {
        let _guard = TransactionGuard::new();
        let mut transaction = Transaction::default();

        loop {
            match f(&mut transaction) {
                Ok(t) => {
                    if transaction.commit() {
                        return TransactionResult::Validated(t);
                    }
                }
                Err(e) => match e {
                    TransactionError::Abort(err) => {
                        transaction.clear();
                        return TransactionResult::Cancelled(err);
                    }
                    TransactionError::Stm(err) => {
                        if let TransactionControl::Abort = control(err) {
                            transaction.clear();
                            return TransactionResult::Abandoned;
                        }

                        #[cfg(feature = "wait-on-retry")]
                        if let StmError::Retry = err {
                            transaction.wait_for_change();
                        }
                    }
                },
            }

            transaction.clear();
        }
    }

    #[cfg(feature = "profiling")]
    pub fn profile_with<T, F>(f: F) -> (T, TransactionTallies)
    where
        F: Fn(&mut Transaction) -> StmClosureResult<T>,
    {
        match Transaction::profile_with_control(|_| TransactionControl::Retry, f) {
            (Some(t), tallies) => (t, tallies),
            (None, _) => unreachable!(),
        }
    }

    #[cfg(feature = "profiling")]
    pub fn profile_with_control<T, F, C>(mut control: C, f: F) -> (Option<T>, TransactionTallies)
    where
        F: Fn(&mut Transaction) -> StmClosureResult<T>,
        C: FnMut(StmError) -> TransactionControl,
    {
        let _guard = TransactionGuard::new();
        let mut transaction = Transaction::default();

        loop {
            transaction
                .tallies
                .n_attempts
                .fetch_add(1, Ordering::Relaxed);
            match f(&mut transaction) {
                Ok(t) => {
                    if transaction.commit() {
                        return (Some(t), transaction.finish_tallies());
                    }
                }
                Err(e) => {
                    transaction.tally_error(e);
                    if let TransactionControl::Abort = control(e) {
                        transaction.clear();
                        return (None, transaction.finish_tallies());
                    }

                    #[cfg(feature = "wait-on-retry")]
                    if let StmError::Retry = e {
                        transaction.wait_for_change();
                    }
                }
            }

            transaction.clear();
        }
    }

    #[cfg(feature = "profiling")]
    pub fn profile_with_err<T, F, E>(f: F) -> (Result<T, E>, TransactionTallies)
    where
        F: Fn(&mut Transaction) -> TransactionClosureResult<T, E>,
    {
        let _guard = TransactionGuard::new();
        let mut transaction = Transaction::default();

        loop {
            transaction
                .tallies
                .n_attempts
                .fetch_add(1, Ordering::Relaxed);
            match f(&mut transaction) {
                Ok(t) => {
                    if transaction.commit() {
                        return (Ok(t), transaction.finish_tallies());
                    }
                }
                Err(e) => match e {
                    TransactionError::Abort(err) => {
                        transaction.clear();
                        return (Err(err), transaction.finish_tallies());
                    }
                    TransactionError::Stm(err) => {
                        transaction.tally_error(err);
                        #[cfg(feature = "wait-on-retry")]
                        if let StmError::Retry = err {
                            transaction.wait_for_change();
                        }
                    }
                },
            }

            transaction.clear();
        }
    }

    #[cfg(feature = "profiling")]
    pub fn profile_with_control_and_err<T, F, C, E>(
        mut control: C,
        f: F,
    ) -> (TransactionResult<T, E>, TransactionTallies)
    where
        F: Fn(&mut Transaction) -> TransactionClosureResult<T, E>,
        C: FnMut(StmError) -> TransactionControl,
    {
        let _guard = TransactionGuard::new();
        let mut transaction = Transaction::default();

        loop {
            transaction
                .tallies
                .n_attempts
                .fetch_add(1, Ordering::Relaxed);
            match f(&mut transaction) {
                Ok(t) => {
                    if transaction.commit() {
                        return (
                            TransactionResult::Validated(t),
                            transaction.finish_tallies(),
                        );
                    }
                }
                Err(e) => match e {
                    TransactionError::Abort(err) => {
                        transaction.clear();
                        return (
                            TransactionResult::Cancelled(err),
                            transaction.finish_tallies(),
                        );
                    }
                    TransactionError::Stm(err) => {
                        transaction.tally_error(err);
                        if let TransactionControl::Abort = control(err) {
                            transaction.clear();
                            return (TransactionResult::Abandoned, transaction.finish_tallies());
                        }

                        #[cfg(feature = "wait-on-retry")]
                        if let StmError::Retry = err {
                            transaction.wait_for_change();
                        }
                    }
                },
            }

            transaction.clear();
        }
    }
}

impl Transaction {
    pub fn read<T: Send + Sync + Any + Clone>(&mut self, var: &TVar<T>) -> StmClosureResult<T> {
        #[cfg(feature = "profiling")]
        self.tallies.n_read.fetch_add(1, Ordering::Relaxed);

        let ctrl = var.control_block().clone();
        if let Some(write) = self.writes.get(&ctrl) {
            #[cfg(feature = "profiling")]
            self.tallies
                .n_read_after_write
                .fetch_add(1, Ordering::Relaxed);
            return Ok(Self::downcast(&write.value));
        }

        let (value, timestamp) = ctrl.load_version();
        if ctrl.is_acquired_by_other(self.id) {
            return Err(StmError::Failure);
        }

        if let Some(read) = self.reads.get(&ctrl) {
            #[cfg(feature = "profiling")]
            self.tallies
                .n_redundant_read
                .fetch_add(1, Ordering::Relaxed);
            if !read.obsolete && read.timestamp != timestamp {
                return Err(StmError::Failure);
            }
        }

        if timestamp > self.clock && !self.extend(timestamp) {
            return Err(StmError::Failure);
        }

        self.reads.insert(
            ctrl,
            ReadEntry {
                timestamp,
                obsolete: false,
            },
        );
        Ok(Self::downcast(&value))
    }

    pub fn write<T: Any + Send + Sync + Clone>(
        &mut self,
        var: &TVar<T>,
        value: T,
    ) -> StmClosureResult<()> {
        #[cfg(feature = "profiling")]
        self.tallies.n_write.fetch_add(1, Ordering::Relaxed);

        let ctrl = var.control_block().clone();
        self.acquire_and_extend(&ctrl)?;
        self.writes.insert(
            ctrl,
            WriteEntry {
                value: Arc::new(value),
            },
        );
        Ok(())
    }

    pub fn modify<T: Any + Send + Sync + Clone, F>(
        &mut self,
        var: &TVar<T>,
        f: F,
    ) -> StmClosureResult<()>
    where
        F: FnOnce(T) -> T,
    {
        let value = self.read(var)?;
        self.write(var, f(value))
    }

    pub fn exchange<T: Any + Send + Sync + Clone>(
        &mut self,
        var: &TVar<T>,
        value: T,
    ) -> StmClosureResult<T> {
        let old = self.read(var)?;
        self.write(var, value)?;
        Ok(old)
    }

    pub fn or<T, F1, F2>(&mut self, first: F1, second: F2) -> StmClosureResult<T>
    where
        F1: Fn(&mut Transaction) -> StmClosureResult<T>,
        F2: Fn(&mut Transaction) -> StmClosureResult<T>,
    {
        let reads_before = self.reads.clone();
        let writes_before = self.writes.clone();

        match first(self) {
            Err(StmError::Retry) => {
                let first_reads = std::mem::replace(&mut self.reads, reads_before);
                let first_writes = std::mem::replace(&mut self.writes, writes_before);
                self.release_write_set(first_writes);

                let second_result = second(self);
                if !matches!(second_result, Err(StmError::Failure)) {
                    self.combine_obsolete(first_reads);
                }
                second_result
            }
            x => x,
        }
    }
}

impl Transaction {
    fn downcast<T: Any + Clone>(var: &ArcAny) -> T {
        match var.downcast_ref::<T>() {
            Some(s) => s.clone(),
            None => unreachable!("TVar has wrong type"),
        }
    }

    fn acquire_and_extend(&mut self, ctrl: &Arc<VarControlBlock>) -> StmClosureResult<()> {
        if !ctrl.try_acquire_for(self.id) {
            return Err(StmError::Failure);
        }

        let (_, timestamp) = ctrl.load_version();
        if timestamp > self.clock && !self.extend(timestamp) {
            ctrl.release_from(self.id);
            return Err(StmError::Failure);
        }

        Ok(())
    }

    fn extend(&mut self, timestamp: u64) -> bool {
        for (ctrl, read) in &self.reads {
            if read.obsolete {
                continue;
            }

            let (_, current_timestamp) = ctrl.load_version();
            if ctrl.is_acquired_by_other(self.id) || current_timestamp != read.timestamp {
                return false;
            }
        }

        self.clock = self.clock.max(timestamp);
        true
    }

    fn combine_obsolete(&mut self, reads: ReadSet) {
        for (ctrl, mut read) in reads {
            read.obsolete = true;
            match self.reads.entry(ctrl) {
                Entry::Vacant(entry) => {
                    entry.insert(read);
                }
                Entry::Occupied(_) => {}
            }
        }
    }

    fn clear(&mut self) {
        self.release_writes();
        self.reads.clear();
        self.writes.clear();
    }

    fn release_writes(&mut self) {
        let writes = std::mem::take(&mut self.writes);
        self.release_write_set(writes);
    }

    fn release_write_set(&self, writes: WriteSet) {
        for (ctrl, _) in writes {
            ctrl.release_from(self.id);
        }
    }

    #[cfg(feature = "wait-on-retry")]
    fn wait_for_change(&mut self) {
        let ctrl = Arc::new(ControlBlock::new());
        let mut observed = Vec::with_capacity(self.reads.len());
        let mut blocking = true;

        for (var, read) in &self.reads {
            var.wait(&ctrl);
            let (_, timestamp) = var.load_version();
            observed.push(var.clone());
            if timestamp != read.timestamp {
                blocking = false;
            }
        }

        if blocking {
            ctrl.wait();
        }

        for var in &observed {
            var.set_dead();
        }
    }

    pub(crate) fn commit(&mut self) -> bool {
        if !self.extend(self.clock) {
            self.release_writes();
            return false;
        }

        if !self.writes.is_empty() {
            self.clock = self.clock.saturating_add(1);
            let commit_timestamp = self.clock;

            for (ctrl, write) in &self.writes {
                let mut state = ctrl.state.write();
                state.value = write.value.clone();
                state.timestamp = commit_timestamp;
            }

            #[cfg(feature = "wait-on-retry")]
            for ctrl in self.writes.keys() {
                ctrl.wake_all();
            }
        }

        self.release_writes();
        self.reads.clear();
        true
    }

    #[cfg(feature = "profiling")]
    fn tally_error(&self, err: StmError) {
        match err {
            StmError::Failure => {
                self.tallies.n_error.fetch_add(1, Ordering::Relaxed);
            }
            StmError::Retry => {
                self.tallies.n_retry.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    #[cfg(feature = "profiling")]
    fn finish_tallies(&self) -> TransactionTallies {
        let tallies = TransactionTallies::default();
        tallies.n_attempts.store(
            self.tallies.n_attempts.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        tallies.n_retry.store(
            self.tallies.n_retry.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        tallies.n_error.store(
            self.tallies.n_error.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        tallies.n_read.store(
            self.tallies.n_read.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        tallies.n_redundant_read.store(
            self.tallies.n_redundant_read.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        tallies.n_read_after_write.store(
            self.tallies.n_read_after_write.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        tallies.n_write.store(
            self.tallies.n_write.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        tallies
    }
}
