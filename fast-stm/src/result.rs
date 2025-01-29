#[derive(Eq, PartialEq, Clone, Copy, Debug, thiserror::Error)]
pub enum StmError {
    /// The call failed, because a variable, the computation
    /// depends on, has changed.
    #[error("Transaction failure signal")]
    Failure,

    /// `retry` was called.
    ///
    /// It may block until at least one read variable has changed.
    #[error("Transaction retry signal")]
    Retry,
}

/// `StmResult` is a result of a single step of a STM calculation.
///
/// It informs of success or the type of failure. Normally you should not use
/// it directly. Especially recovering from an error, e.g. by using `action1.or(action2)`
/// can break the semantics of stm, and cause delayed wakeups or deadlocks.
///
/// For the later case, there is the `transaction.or(action1, action2)`, that
/// is safe to use.
pub type StmResult<T> = Result<T, StmError>;

#[derive(Eq, PartialEq, Clone, Copy, Debug, thiserror::Error)]
pub enum TransactionError<E> {
    /// Failed due to [`StmError`].
    Stm(#[from] StmError),
    /// `abort` was called.
    ///
    /// The transaction will be aborted and the error returned.
    Abort(E),
}

/// Result of a transaction with failure potential
pub type TransactionResult<T, E> = Result<T, TransactionError<E>>;
