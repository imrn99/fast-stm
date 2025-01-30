// regular transaction

/// Error of a single step of a transaction.
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

/// Return type of a transaction body.
///
/// It is the result of a single step of a STM calculation. It informs of success or the type
/// of failure. Normally you should not use it directly. Especially recovering from an error,
/// e.g. by using `action1.or(action2)` can break the semantics of stm, and cause delayed
/// wakeups or deadlocks.
///
/// For the later case, there is the `transaction.or(action1, action2)`, that
/// is safe to use.
pub type StmClosureResult<T> = Result<T, StmError>;

// fallible transaction

/// Error of a single step of a fallible transaction.
#[derive(Eq, PartialEq, Clone, Copy, Debug, thiserror::Error)]
pub enum TransactionError<E> {
    /// Failed due to a regular [`StmError`].
    Stm(#[from] StmError),
    /// Failed due to manual cancelling (e.g. a call to `abort` in the transaction's body).
    Abort(E),
}

/// Return type of a fallible transaction body.
pub type TransactionClosureResult<T, E> = Result<T, TransactionError<E>>;

/// Result of a fallible transaction.
///
/// A given transaction can finish in three different ways:
/// - it is validated, and possibly returns an output value,
/// - it is manually cancelled, and possibly returns a user-defined error,
/// - it is cancelled through regular STM control flow.
///
/// Each variant of this enum represents a case. All of the associated methods behave
/// like their equivalent for [`std::result::Result`].
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
#[must_use = "this `TransactionResult` may model an error, which should be handled"]
pub enum TransactionResult<T, E> {
    /// Transaction completed successfully.
    Validated(T),
    /// Transaction was manually aborted.
    Cancelled(E),
    /// Transaction was aborted.
    Failed,
}

impl<T, E> TransactionResult<T, E> {
    /// Returns `true` if the result is [`Validated`][Self::Validated].
    pub fn is_validated(&self) -> bool {
        matches!(self, Self::Validated(_))
    }

    /// Returns `true` if the result is [`Validated`][Self::Validated]
    /// and the value inside of it matches a predicate.
    pub fn is_validated_and(self, f: impl FnOnce(T) -> bool) -> bool {
        match self {
            Self::Validated(t) => f(t),
            Self::Cancelled(_) | Self::Failed => false,
        }
    }

    /// Returns `true` if the result is [`Cancelled`][Self::Cancelled].
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled(_))
    }

    /// Returns `true` if the result is [`Cancelled`][Self::Cancelled]
    /// and the value inside of it matches a predicate.
    pub fn is_cancelled_and(self, f: impl FnOnce(E) -> bool) -> bool {
        match self {
            Self::Cancelled(e) => f(e),
            Self::Validated(_) | Self::Failed => false,
        }
    }

    /// Converts `self: TransactionResult<T, E>` to [`Option<T>`], consuming `self`,
    /// and discarding the error, if any.
    pub fn validated(self) -> Option<T> {
        match self {
            Self::Validated(t) => Some(t),
            Self::Cancelled(_) | Self::Failed => None,
        }
    }

    /// Converts `self: TransactionResult<T, E>` to [`Option<E>`], consuming `self`,
    /// and discarding the success value, if any.    
    pub fn cancelled(self) -> Option<E> {
        match self {
            Self::Cancelled(e) => Some(e),
            Self::Validated(_) | Self::Failed => None,
        }
    }

    /// Returns `true` if the result is [`Failed`][Self::Failed].
    pub fn failed(self) -> bool {
        matches!(self, Self::Failed)
    }

    /// Returns the contained [`Validated`][Self::Validated] value, consuming `self`.
    ///
    /// # Panics
    ///
    /// Panics if the value is a [`Cancelled`][Self::Cancelled] or [`Failed`][Self::Failed],
    /// with a panic message including the passed message, and the content of
    /// [`Cancelled`][Self::Cancelled] if applicable.
    pub fn expect(self, msg: &str) -> T
    where
        E: std::fmt::Debug,
    {
        match self {
            Self::Validated(t) => t,
            Self::Cancelled(e) => panic!("{msg}: {e:?}"),
            Self::Failed => panic!("{msg}"),
        }
    }

    /// Returns the contained [`Cancelled`][Self::Cancelled] error, consuming `self`.
    ///
    /// # Panics
    ///
    /// Panics if the value is a [`Validated`][Self::Validated] or [`Failed`][Self::Failed],
    /// with a panic message including the passed message, and the content of
    /// [`Validated`][Self::Validated] if applicable.
    pub fn expect_err(self, msg: &str) -> E
    where
        T: std::fmt::Debug,
    {
        match self {
            Self::Validated(t) => panic!("{msg}: {t:?}"),
            Self::Cancelled(e) => e,
            Self::Failed => panic!("{msg}"),
        }
    }

    /// Returns the contained [`Validated`][Self::Validated] value, consuming `self`.
    ///
    /// # Panics
    ///
    /// Panics if the value is a [`Cancelled`][Self::Cancelled] or [`Failed`][Self::Failed], with
    /// a panic message specified by the content of [`Cancelled`][Self::Cancelled] if applicable.    
    pub fn unwrap(self) -> T
    where
        E: std::fmt::Debug,
    {
        match self {
            Self::Validated(t) => t,
            Self::Cancelled(e) => {
                panic!("called `TransactionResult::unwrap()` on a `Cancelled` value: {e:?}")
            }
            Self::Failed => panic!("called `TransactionResult::unwrap()` on a `Failed` value"),
        }
    }

    /// Returns the contained [`Cancelled`][Self::Cancelled] error, consuming `self`.
    ///
    /// # Panics
    ///
    /// Panics if the value is a [`Validated`][Self::Validated] or [`Failed`][Self::Failed], with
    /// a panic message specified by the content of [`Validated`][Self::Validated] if applicable.    
    pub fn unwrap_err(self) -> E
    where
        T: std::fmt::Debug,
    {
        match self {
            Self::Validated(t) => {
                panic!("called `TransactionResult::unwrap_err()` on a `Validated` value: {t:?}")
            }
            Self::Cancelled(e) => e,
            Self::Failed => panic!("called `TransactionResult::unwrap_err()` on a `Failed` value"),
        }
    }

    /// Returns the contained [`Validated`][Self::Validated] value or a default value,
    /// consuming `self`.
    ///
    /// If the value is a [`Cancelled`][Self::Cancelled] or [`Failed`][Self::Failed], the `Default`
    /// implementation of `T` is called to return a value.    
    pub fn unwrap_or_default(self) -> T
    where
        T: Default,
    {
        match self {
            Self::Validated(t) => t,
            Self::Cancelled(_) | Self::Failed => Default::default(),
        }
    }
}
