/// Error of a single step of a transaction.
#[derive(Eq, PartialEq, Clone, Copy, Debug, thiserror::Error)]
pub enum StmError {
    /// The call failed because the transaction observed a conflict.
    #[error("Transaction failure signal")]
    Failure,

    /// `retry` was called.
    #[error("Transaction retry signal")]
    Retry,
}

/// Return type of a transaction body.
pub type StmClosureResult<T> = Result<T, StmError>;

/// Error of a single step of a fallible transaction.
#[derive(Eq, PartialEq, Clone, Copy, Debug, thiserror::Error)]
pub enum TransactionError<E> {
    /// Failed due to a regular [`StmError`].
    Stm(#[from] StmError),
    /// Failed due to manual cancelling.
    Abort(E),
}

/// Return type of a fallible transaction body.
pub type TransactionClosureResult<T, E> = Result<T, TransactionError<E>>;

/// Result of a fallible transaction.
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
#[must_use = "this `TransactionResult` may model an error, which should be handled"]
pub enum TransactionResult<T, E> {
    /// Transaction completed successfully.
    Validated(T),
    /// Transaction was manually aborted.
    Cancelled(E),
    /// Transaction was abandoned through standard STM control flow.
    Abandoned,
}

impl<T, E> TransactionResult<T, E> {
    pub fn is_validated(&self) -> bool {
        matches!(self, Self::Validated(_))
    }

    pub fn is_validated_and(self, f: impl FnOnce(T) -> bool) -> bool {
        match self {
            Self::Validated(t) => f(t),
            Self::Cancelled(_) | Self::Abandoned => false,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled(_))
    }

    pub fn is_cancelled_and(self, f: impl FnOnce(E) -> bool) -> bool {
        match self {
            Self::Cancelled(e) => f(e),
            Self::Validated(_) | Self::Abandoned => false,
        }
    }

    pub fn validated(self) -> Option<T> {
        match self {
            Self::Validated(t) => Some(t),
            Self::Cancelled(_) | Self::Abandoned => None,
        }
    }

    pub fn cancelled(self) -> Option<E> {
        match self {
            Self::Cancelled(e) => Some(e),
            Self::Validated(_) | Self::Abandoned => None,
        }
    }

    pub fn failed(self) -> bool {
        matches!(self, Self::Abandoned)
    }

    pub fn expect(self, msg: &str) -> T
    where
        E: std::fmt::Debug,
    {
        match self {
            Self::Validated(t) => t,
            Self::Cancelled(e) => panic!("{msg}: {e:?}"),
            Self::Abandoned => panic!("{msg}"),
        }
    }

    pub fn expect_err(self, msg: &str) -> E
    where
        T: std::fmt::Debug,
    {
        match self {
            Self::Validated(t) => panic!("{msg}: {t:?}"),
            Self::Cancelled(e) => e,
            Self::Abandoned => panic!("{msg}"),
        }
    }

    pub fn unwrap(self) -> T
    where
        E: std::fmt::Debug,
    {
        match self {
            Self::Validated(t) => t,
            Self::Cancelled(e) => {
                panic!("called `TransactionResult::unwrap()` on a `Cancelled` value: {e:?}")
            }
            Self::Abandoned => {
                panic!("called `TransactionResult::unwrap()` on a `Abandoned` value")
            }
        }
    }

    pub fn unwrap_err(self) -> E
    where
        T: std::fmt::Debug,
    {
        match self {
            Self::Validated(t) => {
                panic!("called `TransactionResult::unwrap_err()` on a `Validated` value: {t:?}")
            }
            Self::Cancelled(e) => e,
            Self::Abandoned => {
                panic!("called `TransactionResult::unwrap_err()` on a `Abandoned` value")
            }
        }
    }

    pub fn unwrap_or_default(self) -> T
    where
        T: Default,
    {
        match self {
            Self::Validated(t) => t,
            Self::Cancelled(_) | Self::Abandoned => Default::default(),
        }
    }
}
