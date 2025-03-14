//! This library implements
//! [software transactional memory](https://en.wikipedia.org/wiki/Software_transactional_memory),
//! often abbreviated with STM.
//!
//! It is designed closely to haskells STM library. Read Simon Marlow's
//! *Parallel and Concurrent Programming in Haskell*
//! for more info. Especially the chapter about
//! Performance is also important for using STM in rust.
//!
//! With locks the sequential composition of two
//! two threadsafe actions is no longer threadsafe because
//! other threads may interfer in between of these actions.
//! Applying a third lock to protect both may lead to common sources of errors
//! like deadlocks or race conditions.
//!
//! Unlike locks Software transactional memory is composable.
//! It is typically implemented by writing all read and write
//! operations in a log. When the action has finished and
//! all the used `TVar`s are consistent, the writes are commited as
//! a single atomic operation.
//! Otherwise the computation repeats. This may lead to starvation,
//! but avoids common sources of bugs.
//!
//! Panicing within STM does not poison the `TVar`s. STM ensures consistency by
//! never committing on panic.
//!
//! # Usage
//!
//! You should only use the functions that are transaction-safe.
//! Transaction-safe functions don't have side effects, except those provided by `TVar`.
//! Mutexes and other blocking mechanisms are especially dangerous, because they can
//! interfere with the internal locking scheme of the transaction and therefore
//! cause deadlocks.
//!
//! Note, that Transaction-safety does *not* mean safety in the rust sense, but is a
//! subset of allowed behavior. Even if code is not transaction-safe, no segmentation
//! faults will happen.
//!
//! You can run the top-level atomic operation by calling `atomically`.
//!
//!
//! ```
//! # use fast_stm::atomically;
//! atomically(|trans| {
//!     // some action
//!     // return value as `Result`, for example
//!     Ok(42)
//! });
//! ```
//!
//! Nested calls to `atomically` are not allowed. A run-time check prevents this.
//! Instead of using atomically internally, add a `&mut Transaction` parameter and
//! return `StmResult`.
//!
//! Use ? on `StmResult`, to propagate a transaction error through the system.
//! Do not handle the error yourself.
//!
//! ```
//! # use fast_stm::{atomically, TVar};
//! let var = TVar::new(0);
//!
//! let x = atomically(|trans| {
//!     var.write(trans, 42)?; // Pass failure to parent.
//!     var.read(trans) // Return the value saved in var.
//! });
//!
//! println!("var = {}", x);
//! // var = 42
//!
//! ```
//!
//! # Transaction safety
//!
//! Software transactional memory is completely safe in the rust sense, so
//! undefined behavior will never occur.
//! Still there are multiple rules that
//! you should obey when dealing with software transactional memory.
//!
//! * Don't run code with side effects, especially no IO-code.
//!   Transactions repeat in failure cases. Using IO would repeat this IO-code.
//!   Return a closure if you have to.
//! * Don't handle `StmResult` yourself.
//!   Use `Transaction::or` to combine alternative paths and `optionally` to check if an inner
//!   function has failed. Always use `?` and
//!   never ignore a `StmResult`.
//! * Don't run `atomically` inside of another. `atomically` is designed to have side effects
//!   and will therefore break transaction safety.
//!   Nested calls are detected at runtime and handled with panicking.
//!   When you use STM in the inner of a function, then
//!   express it in the public interface, by taking `&mut Transaction` as parameter and
//!   returning `StmResult<T>`. Callers can safely compose it into
//!   larger blocks.
//! * Don't mix locks and transactions. Your code will easily deadlock or slow
//!   down unpredictably.
//! * Don't use inner mutability to change the content of a `TVar`.
//!
//! Panicking in a transaction is transaction-safe. The transaction aborts and
//! all changes are discarded. No poisoning or half written transactions happen.
//!
//! # Speed
//!
//! Generally keep your atomic blocks as small as possible, because
//! the more time you spend, the more likely it is, to collide with
//! other threads. For STM, reading `TVar`s is quite slow, because it
//! needs to look them up in the log every time.
//! Every used `TVar` increases the chance of collisions. Therefore you should
//! keep the amount of accessed variables as low as needed.
//!

// Extra linting with exceptions
#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::should_panic_without_expect)]

extern crate parking_lot;

mod result;
mod transaction;
mod tvar;

#[cfg(test)]
mod test;

pub use result::*;
pub use transaction::Transaction;
pub use transaction::TransactionControl;
pub use tvar::TVar;

/// Convert a `TransactionClosureResult<T, E_A>` to `TransactionClosureResult<T, E_B>`.
///
/// This macro is used to cleanly write transactions where multiple kind of errors are
/// possible during execution. The macro will not fail as long as the specified target
/// error `$to` implements `From<E>`, `E` being the error possibly returned by `$op`.
/// It expands to:
///
/// ```ignore
/// $op.map_err(|e| match e {
///         fast_stm::TransactionError::Abort(e) => fast_stm::TransactionError::Abort($to::from(e)),
///         fast_stm::TransactionError::Stm(e) => fast_stm::TransactionError::Stm(e),
///     })?
/// ```
///
/// # Example
///
/// ```rust
/// # use fast_stm::{abort, atomically_with_err, try_or_coerce, Transaction, TransactionClosureResult};
///
/// struct Error1;
/// struct Error2;
///
/// impl From<Error1> for Error2 {
///     fn from(e: Error1) -> Self {
///         Error2
///     }
/// }
///
/// fn op1(trans: &mut Transaction) -> TransactionClosureResult<(), Error1> {
///     // ...
///     Ok(())
/// }
///
/// fn op2(trans: &mut Transaction) -> TransactionClosureResult<(), Error2> {
///     // ...
///     Ok(())
/// }
///
/// let res: Result<(), Error2> = atomically_with_err(|trans| {
///     try_or_coerce!(op1(trans), Error2);
///     op2(trans)?;   
///     Ok(())
/// });
/// ```
#[macro_export]
macro_rules! try_or_coerce {
    ($op: expr, $to: ident) => {
        $op.map_err(|e| match e {
            $crate::TransactionError::Abort(e) => $crate::TransactionError::Abort($to::from(e)),
            $crate::TransactionError::Stm(e) => $crate::TransactionError::Stm(e),
        })?
    };
}

#[inline]
/// Call `abort` to abort a transaction and pass the error as the return value.
///
/// # Examples
///
/// ```
/// # use fast_stm::*;
/// struct MyError;
///
/// let execute_once: Result<u32, _> = atomically_with_err(|_| {
///     abort(MyError)
/// });
///
/// assert!(execute_once.is_err());
/// ```
pub fn abort<T, E>(e: E) -> TransactionClosureResult<T, E> {
    Err(TransactionError::Abort(e))
}

#[inline]
/// Call `retry` to abort an operation and run the whole transaction again.
///
/// Semantically `retry` allows spin-lock-like behavior, but the library
/// blocks until one of the used `TVar`s has changed, to keep CPU-usage low.
///
/// `Transaction::or` allows to define alternatives. If the first function
/// wants to retry, then the second one has a chance to run.
///
/// # Examples
///
/// ```no_run
/// # use fast_stm::*;
/// let infinite_retry: i32 = atomically(|_| retry());
/// ```
pub fn retry<T>() -> StmClosureResult<T> {
    Err(StmError::Retry)
}

/// Run a function atomically by using Software Transactional Memory.
/// It calls to `Transaction::with` internally, but is more explicit.
pub fn atomically<T, F>(f: F) -> T
where
    F: Fn(&mut Transaction) -> StmClosureResult<T>,
{
    Transaction::with(f)
}

/// Run a function atomically by using Software Transactional Memory.
/// It calls to `Transaction::with_err` internally, but is more explicit.
pub fn atomically_with_err<T, E, F>(f: F) -> Result<T, E>
where
    F: Fn(&mut Transaction) -> TransactionClosureResult<T, E>,
{
    Transaction::with_err(f)
}

#[inline]
/// Unwrap `Option` or call retry if it is `None`.
///
/// `optionally` is the inverse of `unwrap_or_retry`.
///
/// # Example
///
/// ```
/// # use fast_stm::*;
/// let x = TVar::new(Some(42));
///
/// atomically(|tx| {
///         let inner = unwrap_or_retry(x.read(tx)?)?;
///         assert_eq!(inner, 42); // inner is always 42.
///         Ok(inner)
///     }
/// );
/// ```
pub fn unwrap_or_retry<T>(option: Option<T>) -> StmClosureResult<T> {
    match option {
        Some(x) => Ok(x),
        None => retry(),
    }
}

#[inline]
/// Retry until `cond` is true.
///
/// # Example
///
/// ```
/// # use fast_stm::*;
/// let var = TVar::new(42);
///
/// let x = atomically(|tx| {
///     let v = var.read(tx)?;
///     guard(v==42)?;
///     // v is now always 42.
///     Ok(v)
/// });
/// assert_eq!(x, 42);
/// ```
pub fn guard(cond: bool) -> StmClosureResult<()> {
    if cond {
        Ok(())
    } else {
        retry()
    }
}

#[inline]
/// Optionally run a transaction `f`. If `f` fails with a `retry()`, it does
/// not cancel the whole transaction, but returns `None`.
///
/// Note that `optionally` does not always recover the function, if
/// inconsistencies where found.
///
/// `unwrap_or_retry` is the inverse of `optionally`.
///
/// # Example
///
/// ```
/// # use fast_stm::*;
/// let x:Option<i32> = atomically(|tx|
///     optionally(tx, |_| retry()));
/// assert_eq!(x, None);
/// ```
pub fn optionally<T, F>(tx: &mut Transaction, f: F) -> StmClosureResult<Option<T>>
where
    F: Fn(&mut Transaction) -> StmClosureResult<T>,
{
    tx.or(|t| f(t).map(Some), |_| Ok(None))
}

#[cfg(test)]
mod test_lib {
    use super::*;

    #[test]
    fn infinite_retry() {
        let terminated = test::terminates(300, || {
            let _infinite_retry: i32 = atomically(|_| retry());
        });
        assert!(!terminated);
    }

    #[test]
    fn stm_nested() {
        let var = TVar::new(0);

        let x = atomically(|tx| {
            var.write(tx, 42)?;
            var.read(tx)
        });

        assert_eq!(42, x);
    }

    /// Run multiple threads.
    ///
    /// Thread 1: Read a var, block until it is not 0 and then
    /// return that value.
    ///
    /// Thread 2: Wait a bit. Then write a value.
    ///
    /// Check if Thread 1 is woken up correctly and then check for
    /// correctness.
    #[test]
    fn threaded() {
        use std::thread;
        use std::time::Duration;

        let var = TVar::new(0);
        // Clone for other thread.
        let varc = var.clone();

        let x = test::async_test(
            800,
            move || {
                atomically(|tx| {
                    let x = varc.read(tx)?;
                    if x == 0 {
                        retry()
                    } else {
                        Ok(x)
                    }
                })
            },
            || {
                thread::sleep(Duration::from_millis(100));

                atomically(|tx| var.write(tx, 42));
            },
        )
        .unwrap();

        assert_eq!(42, x);
    }

    /// test if a STM calculation is rerun when a Var changes while executing
    #[test]
    fn read_write_interfere() {
        use std::thread;
        use std::time::Duration;

        // create var
        let var = TVar::new(0);
        let varc = var.clone(); // Clone for other thread.

        // spawn a thread
        let t = thread::spawn(move || {
            atomically(|tx| {
                // read the var
                let x = varc.read(tx)?;
                // ensure that x varc changes in between
                thread::sleep(Duration::from_millis(500));

                // write back modified data this should only
                // happen when the value has not changed
                varc.write(tx, x + 10)
            });
        });

        // ensure that the thread has started and already read the var
        thread::sleep(Duration::from_millis(100));

        // now change it
        atomically(|tx| var.write(tx, 32));

        // finish and compare
        let _ = t.join();
        assert_eq!(42, var.read_atomic());
    }

    #[test]
    fn or_simple() {
        let var = TVar::new(42);

        let x = atomically(|tx| tx.or(|_| retry(), |tx| var.read(tx)));

        assert_eq!(x, 42);
    }

    /// A variable should not be written,
    /// when another branch was taken
    #[test]
    fn or_nocommit() {
        let var = TVar::new(42);

        let x = atomically(|tx| {
            tx.or(
                |tx| {
                    var.write(tx, 23)?;
                    retry()
                },
                |tx| var.read(tx),
            )
        });

        assert_eq!(x, 42);
    }

    #[test]
    fn or_nested_first() {
        let var = TVar::new(42);

        let x = atomically(|tx| tx.or(|tx| tx.or(|_| retry(), |_| retry()), |tx| var.read(tx)));

        assert_eq!(x, 42);
    }

    #[test]
    fn or_nested_second() {
        let var = TVar::new(42);

        let x = atomically(|tx| tx.or(|_| retry(), |t| t.or(|t2| var.read(t2), |_| retry())));

        assert_eq!(x, 42);
    }

    #[test]
    fn unwrap_some() {
        let x = Some(42);
        let y = atomically(|_| unwrap_or_retry(x));
        assert_eq!(y, 42);
    }

    #[test]
    fn unwrap_none() {
        let x: Option<i32> = None;
        assert_eq!(unwrap_or_retry(x), retry());
    }

    #[test]
    fn guard_true() {
        let x = guard(true);
        assert_eq!(x, Ok(()));
    }

    #[test]
    fn guard_false() {
        let x = guard(false);
        assert_eq!(x, retry());
    }

    #[test]
    fn optionally_succeed() {
        let x = atomically(|t| optionally(t, |_| Ok(42)));
        assert_eq!(x, Some(42));
    }

    #[test]
    fn optionally_fail() {
        let x: Option<i32> = atomically(|t| optionally(t, |_| retry()));
        assert_eq!(x, None);
    }
}
