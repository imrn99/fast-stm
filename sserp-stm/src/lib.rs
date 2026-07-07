//! # sserp-stm
//!
//! `sserp-stm` is a Rust implementation of
//! [software transactional memory](https://en.wikipedia.org/wiki/Software_transactional_memory)
//! using the SSER+ algorithm from
//! [Boosting Transactional Memory with Stricter Serializability](https://doi.org/10.1007/978-3-319-92408-3_11)
//! by Sutra, Marlier, Schiavoni, and Trahay.
//!
//! ## STM
//!
//! Users who wish to familiarize themselves with the mechanism can skim through the following
//! documents:
//!
//! - Dedicated STM chapter of [_Real World Haskell_](https://wiki.haskell.org/Real_World_Haskell) for
//!   a quick intuitive introduction
//! - [_Software Transactional Memory_, Shavit et al., 1997](https://doi.org/10.1007/s004460050028)
//! - [_On the correctness of transactional memory_, Guerraoui et al., 2008](https://dl.acm.org/doi/10.1145/1345206.1345233)
//!
//! With locks, the sequential composition of two threadsafe actions is no longer
//! threadsafe because other threads may interfere between those actions. Applying a
//! third lock to protect both may lead to common sources of errors like deadlocks
//! or race conditions.
//!
//! Unlike locks, software transactional memory is composable. It is typically
//! implemented by writing all read and write operations in a log. When the action
//! has finished and all the used `TVar`s are consistent, the writes are committed
//! as a single atomic operation. Otherwise the computation repeats. This may lead
//! to starvation, but avoids common sources of bugs.
//!
//! Panicking within STM does not poison the `TVar`s. STM ensures consistency by
//! never committing on panic.
//!
//! ## Features
//!
//! This crate exposes features that can tweak implementation behavior:
//!
//! - `wait-on-retry` - enabled by default. If `retry` is called explicitly in a
//!   transaction, the thread waits for one of the variables read in the initial
//!   transaction to change before attempting the computation again.
//! - `hash-registers` - NOT YET IMPLEMENTED
//!
//! Only `wait-on-retry` is enabled by default.
//!
//! Two additional features are provided for instrumentation:
//!   
//! - `profiling` - add event counters to transactions and expose
//!   `profile_atomically` / `profile_atomically_with_err`.
//! - `bench` - expose manual transaction initialization and commit helpers used by
//!   the repository's benchmarks.
//!
//!
//! ## Usage
//!
//! You should only use the functions that are safe to use.
//!
//! Do not have side effects except for the atomic variables from this library.
//! Especially a mutex or other blocking mechanisms inside software transactional
//! memory is dangerous.
//!
//! You can run the top-level atomic operation by calling `atomically`.
//!
//! ```rust
//! use sserp_stm::atomically;
//!
//! atomically(|_tx| {
//!     // some action
//!     // return value as `Result`, for example
//!     Ok(42)
//! });
//! ```
//!
//! Calls to `atomically` should not be nested.
//!
//! For running an atomic operation inside of another, pass a mutable reference to a
//! `Transaction` and use `?` on the result. You should not handle the error
//! yourself, because it breaks consistency.
//!
//! ```rust
//! use sserp_stm::{atomically, TVar};
//!
//! let var = TVar::new(0);
//!
//! let x = atomically(|tx| {
//!     var.write(tx, 42)?;
//!     var.read(tx)
//! });
//!
//! println!("var = {}", x);
//! ```
//!
//! ## STM safety
//!
//! Software transactional memory is completely safe in the terms that Rust
//! considers safe. Still there are multiple rules that you should obey when
//! dealing with software transactional memory:
//!
//! - Do not run code with side effects, especially no IO-code, because STM repeats
//!   the computation when it detects inconsistent state. Return a closure if you
//!   have to.
//! - Do not handle the error types yourself, unless you absolutely know what you
//!   are doing. Use `Transaction::or` to combine alternative paths. Always use `?`
//!   and never ignore a `StmResult`.
//! - Do not run `atomically` inside of another. `atomically` is designed to have
//!   side effects and will therefore break STM's assumptions. Nested calls are
//!   detected at runtime and handled with panic. When you use STM in the inner of a
//!   function, express it in the public interface by taking `&mut Transaction` as a
//!   parameter and returning `StmResult<T>`. Callers can safely compose it into
//!   larger blocks.
//! - Do not mix locks and transactions. Your code will easily deadlock or slow
//!   unpredictably.
//! - Do not use inner mutability to change the content of a `TVar`.
//!
//! ## Speed
//!
//! Generally keep your atomic blocks as small as possible, because the more time
//! you spend, the more likely it is to collide with other threads. Every used
//! `TVar` increases the chance of collisions. Therefore you should keep the amount
//! of accessed variables as low as needed.
//!
//! ## Profiling
//!
//! The `profiling` feature can be enabled to add event counters to transaction. Their values can
//! be retrieved by passing a reference to `TransactionTallies` to the new entry functions:
//! `profile_atomically`, ...
//!
//! <div class="warning">
//!
//! Do not use the `profiling` feature if you are benchmarking execution times. While regular entry
//! functions (`atomically`, `atomically_with_err`) are still available, they internally implement
//! counters without giving public access to their value. This is done to avoid breaking the API
//! when the feature is enabled.
//!
//! </div>

#![allow(unexpected_cfgs)]
#![cfg_attr(nightly, feature(doc_cfg))]
#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::mutable_key_type)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::should_panic_without_expect)]

mod result;
mod transaction;
mod tvar;

#[cfg(test)]
mod test;

pub use result::*;
pub use transaction::Transaction;
pub use transaction::TransactionControl;
pub use tvar::TVar;

#[cfg(feature = "profiling")]
pub use transaction::TransactionTallies;

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
pub fn abort<T, E>(e: E) -> TransactionClosureResult<T, E> {
    Err(TransactionError::Abort(e))
}

#[inline]
pub fn retry<T>() -> StmClosureResult<T> {
    Err(StmError::Retry)
}

pub fn atomically<T, F>(f: F) -> T
where
    F: Fn(&mut Transaction) -> StmClosureResult<T>,
{
    Transaction::with(f)
}

pub fn atomically_with_err<T, E, F>(f: F) -> Result<T, E>
where
    F: Fn(&mut Transaction) -> TransactionClosureResult<T, E>,
{
    Transaction::with_err(f)
}

#[inline]
pub fn unwrap_or_retry<T>(option: Option<T>) -> StmClosureResult<T> {
    match option {
        Some(x) => Ok(x),
        None => retry(),
    }
}

#[inline]
pub fn unwrap_or_abort<T, E>(option: Option<T>, e: E) -> TransactionClosureResult<T, E> {
    match option {
        Some(x) => Ok(x),
        None => abort(e),
    }
}

#[inline]
pub fn guard(cond: bool) -> StmClosureResult<()> {
    if cond {
        Ok(())
    } else {
        retry()
    }
}

#[inline]
pub fn optionally<T, F>(tx: &mut Transaction, f: F) -> StmClosureResult<Option<T>>
where
    F: Fn(&mut Transaction) -> StmClosureResult<T>,
{
    tx.or(|t| f(t).map(Some), |_| Ok(None))
}

#[cfg(feature = "bench")]
pub fn init_transaction() -> Transaction {
    Transaction::default()
}

#[cfg(feature = "bench")]
pub fn commit_transaction(t: &mut Transaction) -> bool {
    t.commit()
}

#[cfg(feature = "profiling")]
pub fn profile_atomically<T, F>(f: F) -> (T, TransactionTallies)
where
    F: Fn(&mut Transaction) -> StmClosureResult<T>,
{
    Transaction::profile_with(f)
}

#[cfg(feature = "profiling")]
pub fn profile_atomically_with_err<T, E, F>(f: F) -> (Result<T, E>, TransactionTallies)
where
    F: Fn(&mut Transaction) -> TransactionClosureResult<T, E>,
{
    Transaction::profile_with_err(f)
}

#[cfg(test)]
mod test_lib {
    use super::*;
    use std::thread;
    use std::time::Duration;

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

    #[test]
    fn threaded() {
        let var = TVar::new(0);
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

    #[test]
    fn read_write_interfere() {
        let var = TVar::new(0);
        let varc = var.clone();

        let t = thread::spawn(move || {
            atomically(|tx| {
                let x = varc.read(tx)?;
                thread::sleep(Duration::from_millis(300));
                varc.write(tx, x + 10)
            });
        });

        thread::sleep(Duration::from_millis(100));
        atomically(|tx| var.write(tx, 32));

        let _ = t.join();
        assert_eq!(42, var.read_atomic());
    }

    #[test]
    fn or_simple() {
        let var = TVar::new(42);

        let x = atomically(|tx| tx.or(|_| retry(), |tx| var.read(tx)));

        assert_eq!(x, 42);
    }

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
        assert_eq!(var.read_atomic(), 42);
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

    #[test]
    fn abort_returns_user_error() {
        #[derive(Debug, Eq, PartialEq)]
        struct MyError;

        let execute_once: Result<u32, _> = atomically_with_err(|_| abort(MyError));
        assert_eq!(execute_once, Err(MyError));
    }

    #[test]
    fn nested_transaction_panics() {
        let result = std::panic::catch_unwind(|| {
            Transaction::with(|_| {
                Transaction::with(|_| Ok(42));
                Ok(1)
            });
        });

        assert!(result.is_err());
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

    #[test]
    fn transaction_with_control_abort_on_single_run() {
        let read = TVar::new(42);

        let x = Transaction::with_control(|_| TransactionControl::Abort, |tx| read.read(tx));

        assert_eq!(x, Some(42));
    }

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

    #[test]
    fn stale_redundant_read_fails() {
        let var = TVar::new(0);
        let mut tx = Transaction::default();

        assert_eq!(tx.read(&var), Ok(0));
        var.write_atomic(1);

        assert_eq!(tx.read(&var), Err(StmError::Failure));
    }

    #[test]
    fn locked_read_fails() {
        let var = TVar::new(0);
        let mut writer = Transaction::default();
        let mut reader = Transaction::default();

        writer.write(&var, 1).unwrap();

        assert_eq!(reader.read(&var), Err(StmError::Failure));
    }

    #[test]
    fn write_write_conflict_fails() {
        let var = TVar::new(0);
        let mut tx1 = Transaction::default();
        let mut tx2 = Transaction::default();

        tx1.write(&var, 1).unwrap();

        assert_eq!(tx2.write(&var, 2), Err(StmError::Failure));
    }

    #[test]
    fn commit_validates_read_set() {
        let var = TVar::new(0);
        let mut tx = Transaction::default();

        assert_eq!(tx.read(&var), Ok(0));
        var.write_atomic(1);

        assert!(!tx.commit());
        assert_eq!(var.read_atomic(), 1);
    }

    #[test]
    fn modify_and_exchange_work() {
        let var = TVar::new(21);

        atomically(|tx| var.modify(tx, |x| x * 2));
        let old = atomically(|tx| var.exchange(tx, 7));

        assert_eq!(old, 42);
        assert_eq!(var.read_atomic(), 7);
    }

    #[test]
    fn panic_rolls_back_writes() {
        let var = TVar::new(0);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            atomically(|tx| -> StmClosureResult<()> {
                var.write(tx, 1)?;
                panic!("abort transaction");
            });
        }));

        assert!(result.is_err());
        assert_eq!(var.read_atomic(), 0);
    }

    #[test]
    fn test_read_atomic() {
        let var = TVar::new(42);

        assert_eq!(42, var.read_atomic());
    }
}
