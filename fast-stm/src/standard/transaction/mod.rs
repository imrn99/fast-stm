pub mod control_block;
pub mod log_var;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::mem;
use std::sync::Arc;

use parking_lot::RwLockReadGuard;

use self::control_block::ControlBlock;
use self::log_var::LogVar;
use crate::result::{StmError, StmResult};
use crate::tvar::{TVar, VarControlBlock};

type ReadVecIn<'rwl> = RwLockReadGuard<'rwl, Arc<dyn Any + Send + Sync>>;
type WriteVecIn<'b, 'rwl> = (
    &'b Arc<dyn Any + Send + Sync>,
    parking_lot::RwLockWriteGuard<'rwl, Arc<dyn Any + Send + Sync>>,
);
type WrittenIn<'b> = &'b Arc<VarControlBlock>;

thread_local! {
    static TRANSACTION_RUNNING: Cell<bool> = const { Cell::new(false) };

    static TRANSACTION: RefCell<Transaction> = const { RefCell::new(Transaction::new()) };

    static LAST: Cell<Option<std::thread::ThreadId>> = Cell::new(None);
    static READ_VEC: RefCell<Vec<ReadVecIn<'static>>> = RefCell::new(Vec::with_capacity(64));
    static WRITE_VEC: RefCell<Vec<WriteVecIn<'static, 'static>>> = RefCell::new(Vec::with_capacity(64));
    static WRITTEN: RefCell<Vec<WrittenIn<'static>>> = RefCell::new(Vec::with_capacity(64));
}

fn get_read_vec<'r, 'rwl>() -> &'r mut Vec<ReadVecIn<'rwl>> {
    if LAST.get() == None {
        LAST.set(Some(std::thread::current().id()));
    } else {
        assert_eq!(LAST.get(), Some(std::thread::current().id()))
    }
    READ_VEC.with_borrow_mut(|v| unsafe { std::mem::transmute(v) })
}

fn get_write_vec<'r, 'b, 'rwl>() -> &'r mut Vec<WriteVecIn<'b, 'rwl>> {
    WRITE_VEC.with_borrow_mut(|v| unsafe { std::mem::transmute(v) })
}

fn get_written<'r, 'b>() -> &'r mut Vec<WrittenIn<'b>> {
    WRITTEN.with_borrow_mut(|v| unsafe { std::mem::transmute(v) })
}

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

/// Transaction tracks all the read and written variables.
///
/// It is used for checking vars, to ensure atomicity.
pub struct Transaction {
    /// Map of all vars that map the `VarControlBlock` of a var to a `LogVar`.
    /// The `VarControlBlock` is unique because it uses it's address for comparing.
    ///
    /// The logs need to be accessed in a order to prevend dead-locks on locking.
    vars: BTreeMap<Arc<VarControlBlock>, LogVar>,
    read_refs: Vec<Arc<VarControlBlock>>,
}

impl Transaction {
    /// Create a new log.
    ///
    /// Normally you don't need to call this directly.
    /// Use `atomically` instead.
    const fn new() -> Transaction {
        Transaction {
            vars: BTreeMap::new(),
            read_refs: Vec::new(),
        }
    }

    /// Run a function with a transaction.
    ///
    /// It is equivalent to `atomically`.
    pub fn with<T, F>(f: F) -> T
    where
        F: Fn(&mut Transaction) -> StmResult<T>,
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
        F: Fn(&mut Transaction) -> StmResult<T>,
        C: FnMut(StmError) -> TransactionControl,
    {
        // create a log guard for initializing and cleaning up the log
        let _guard = TransactionGuard::new();

        let tmp = TRANSACTION.with_borrow_mut(|transaction| {
            // loop until success
            loop {
                // run the computation
                match f(transaction) {
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
                        if let StmError::Retry = e {
                            transaction.wait_for_change();
                        }
                    }
                }

                // clear log before retrying computation
                transaction.clear();
            }
        });
        TRANSACTION.with_borrow_mut(Transaction::clear);
        tmp
    }

    #[allow(clippy::needless_pass_by_value)]
    /// Perform a downcast on a var.
    fn downcast<T: Any + Clone>(var: Arc<dyn Any>) -> T {
        match var.downcast_ref::<T>() {
            Some(s) => s.clone(),
            None => unreachable!("TVar has wrong type"),
        }
    }

    /// Read a variable and return the value.
    ///
    /// The returned value is not always consistent with the current value of the var,
    /// but may be an outdated or or not yet commited value.
    ///
    /// The used code should be capable of handling inconsistent states
    /// without running into infinite loops.
    /// Just the commit of wrong values is prevented by STM.
    pub fn read<T: Send + Sync + Any + Clone>(&mut self, var: &TVar<T>) -> StmResult<T> {
        let ctrl = var.control_block().clone();
        // Check if the same var was written before.
        let value = match self.vars.entry(ctrl) {
            // If the variable has been accessed before, then load that value.
            Entry::Occupied(mut entry) => entry.get_mut().read(),

            // Else load the variable statically.
            Entry::Vacant(entry) => {
                // Read the value from the var.
                let value = var.read_ref_atomic();

                // Store in in an entry.
                entry.insert(LogVar::Read(value.clone()));
                value
            }
        };

        // For now always succeeds, but that may change later.
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
    ) -> StmResult<()> {
        // box the value
        let boxed = Arc::new(value);

        // new control block
        let ctrl = var.control_block().clone();
        // update or create new entry
        match self.vars.entry(ctrl) {
            Entry::Occupied(mut entry) => entry.get_mut().write(boxed),
            Entry::Vacant(entry) => {
                entry.insert(LogVar::Write(boxed));
            }
        }

        // For now always succeeds, but that may change later.
        Ok(())
    }

    /// Combine two calculations. When one blocks with `retry`,
    /// run the other, but don't commit the changes in the first.
    ///
    /// If both block, `Transaction::or` still waits for `TVar`s in both functions.
    /// Use `Transaction::or` instead of handling errors directly with the `Result::or`.
    /// The later does not handle all the blocking correctly.
    pub fn or<T, F1, F2>(&mut self, first: F1, second: F2) -> StmResult<T>
    where
        F1: Fn(&mut Transaction) -> StmResult<T>,
        F2: Fn(&mut Transaction) -> StmResult<T>,
    {
        // Create a backup of the log.
        let mut copy = Transaction {
            vars: self.vars.clone(),
            read_refs: self.read_refs.clone(),
        };

        // Run the first computation.
        let f = first(self);

        match f {
            // Run other on manual retry call.
            Err(StmError::Retry) => {
                // swap, so that self is the current run
                mem::swap(self, &mut copy);

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

    /// Combine two logs into a single log, to allow waiting for all reads.
    fn combine(&mut self, other: Transaction) {
        // combine reads
        for (var, value) in other.vars {
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
    fn commit(&mut self) -> bool {
        // Use two phase locking for safely writing data back to the vars.

        // First phase: acquire locks.
        // Check for consistency of all the reads and perform
        // an early return if something is not consistent.

        // Created arrays for storing the locks
        // vector of locks.
        let read_vec = get_read_vec(); // Vec::with_capacity(self.vars.len());

        // vector of tuple (value, lock)
        let write_vec = get_write_vec(); // Vec::with_capacity(self.vars.len());

        // vector of written variables
        let written = get_written(); // Vec::with_capacity(self.vars.len());

        read_vec.clear();
        write_vec.clear();
        written.clear();

        for (var, value) in &self.vars {
            // lock the variable and read the value

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
        read_vec.clear();

        for (value, mut lock) in write_vec.drain(..) {
            // Commit value.
            *lock = value.clone();
        }

        for var in written {
            // Unblock all threads waiting for it.
            (*var).wake_all();
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
        let mut log = Transaction::new();
        let var = TVar::new(vec![1, 2, 3, 4]);

        // The variable can be read.
        assert_eq!(&*log.read(&var).unwrap(), &[1, 2, 3, 4]);
    }

    #[test]
    fn write_read() {
        let mut log = Transaction::new();
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
