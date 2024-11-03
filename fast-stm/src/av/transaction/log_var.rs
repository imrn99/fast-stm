use std::any::Any;
use std::sync::Arc;

pub type ArcAny = Arc<dyn Any + Send + Sync>;

/// `LogVar` is used by `Log` to track which `Var` was either read or written or both.
/// Depending on the type, STM has to write, ensure consistency or block on this value.
#[derive(Clone)]
pub enum LogVar {
    /// Var has been read.
    Read(ArcAny),

    /// Var has been written and no dependency on the original exists.
    ///
    /// There is no need to check for consistency.
    Write(ArcAny),

    /// ReadWrite(original value, temporary stored value).
    ///
    /// Var has been read first and then written.
    ///
    /// It needs to be checked for consistency.
    ReadWrite(ArcAny, ArcAny),

    /// Var has been read on blocked path.
    ///
    /// Don't check for consistency, but block on Var,
    /// so that the threat wakes up when the first path
    /// has been unlocked.
    ReadObsolete(ArcAny),

    /// ReadWriteObsolete(original value, temporary stored value).
    ///
    /// Var has been read on blocked path and then written to.
    ///
    /// Don't check for consistency, but block on Var,
    /// so that the threat wakes up when the first path
    /// has been unlocked.
    ReadObsoleteWrite(ArcAny, ArcAny), // Here would be WriteObsolete, but the write onlies can be discarded immediately
                                       // and don't need a representation in the log.
}

impl LogVar {
    /// Read a value and potentially upgrade the state.
    pub fn read(&mut self) -> ArcAny {
        // We do some kind of dance around the borrow checker here.
        // Ideally we only clone the read value and not the write,
        // in order to avoid hitting shared memory as least as possible,
        // but we can not fully avoid it, although these cases happen rarely.
        let this;
        let val;
        match &*self {
            // Use last read value or get written one
            &Self::Read(ref v) | &Self::Write(ref v) | &Self::ReadWrite(_, ref v) => {
                return v.clone();
            }

            Self::ReadObsoleteWrite(w, v) => {
                val = v.clone();
                this = Self::ReadWrite(w.clone(), v.clone());
            }

            // Upgrade to a real Read
            Self::ReadObsolete(v) => {
                val = v.clone();
                this = Self::Read(v.clone());
            }
        };
        *self = this;
        val
    }

    /// Write a value and potentially upgrade the state.
    pub fn write(&mut self, w: ArcAny) {
        let this = self.clone();

        *self = match this {
            Self::Write(_) => Self::Write(w),

            // Register write
            Self::ReadObsolete(r) | Self::ReadObsoleteWrite(r, _) => Self::ReadObsoleteWrite(r, w),

            // Register write
            Self::Read(r) | Self::ReadWrite(r, _) => Self::ReadWrite(r, w),
        };
    }

    /// Turn `self` into an obsolete version.
    pub fn obsolete(self) -> Option<LogVar> {
        self.into_read_value().map(LogVar::ReadObsolete)
    }

    /// Ignore all Write... and get the original value of a Var.
    pub fn into_read_value(self) -> Option<ArcAny> {
        match self {
            LogVar::Read(v)
            | LogVar::ReadWrite(v, _)
            | LogVar::ReadObsolete(v)
            | LogVar::ReadObsoleteWrite(v, _) => Some(v),
            LogVar::Write(_) => None,
        }
    }
}

/// Test if writes are ignored, when a var is set to obsolete.
#[test]
fn test_write_obsolete_ignore() {
    let t = LogVar::Write(Arc::new(42)).obsolete();
    assert!(t.is_none());
}
