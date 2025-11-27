// regular transaction

/// Error of a single step of a transaction.
#[derive(Eq, PartialEq, Clone, Copy, Debug, thiserror::Error)]
pub enum StmError<E> {
    /// `retry` was called.
    ///
    /// It may block until at least one read variable has changed.
    Retry(#[from] RetrySignal),

    /// Failed due to manual cancelling (e.g. a call to `abort` in the transaction's body).
    Abort(#[from] AbortSignal<E>),
}

#[derive(Eq, PartialEq, Clone, Copy, Debug, thiserror::Error)]
#[error("explicit retry: {0}")]
pub struct RetrySignal(pub(crate) &'static str);

#[derive(Eq, PartialEq, Clone, Copy, Debug, thiserror::Error)]
#[error("explicit abort: {0}")]
pub struct AbortSignal<E>(pub(crate) E);

/// Return type of a non-fallible transaction body.
///
/// Transaction of this type may call `retry`, but cannot `abort` with an error.
pub type StmClosureResult<T> = Result<T, RetrySignal>;

/// Return type of a fallible transaction body.
///
/// Transaction of this type may call `retry` and `abort` with an error.
pub type StmFallibleClosureResult<T, E> = Result<T, StmError<E>>;
