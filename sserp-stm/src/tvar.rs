#[cfg(feature = "wait-on-retry")]
use parking_lot::Mutex;
use parking_lot::Mutex as ParkingMutex;
use std::any::Any;
use std::cmp;
use std::fmt::{self, Debug};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
#[cfg(feature = "wait-on-retry")]
use std::sync::Weak;

use super::result::StmClosureResult;
#[cfg(feature = "wait-on-retry")]
use super::transaction::control_block::ControlBlock;
use super::Transaction;

pub type ArcAny = Arc<dyn Any + Send + Sync>;

pub struct Version {
    pub value: ArcAny,
    pub timestamp: u64,
}

/// Type-erased storage and SSER+ metadata for a `TVar`.
pub struct VarControlBlock {
    #[cfg(feature = "wait-on-retry")]
    waiting_threads: Mutex<Vec<Weak<ControlBlock>>>,
    #[cfg(feature = "wait-on-retry")]
    dead_threads: AtomicUsize,
    pub state: ParkingMutex<Version>,
    pub owner: AtomicU64,
}

impl VarControlBlock {
    #[cfg(feature = "wait-on-retry")]
    pub fn new<T>(val: T) -> Arc<VarControlBlock>
    where
        T: Any + Sync + Send,
    {
        Arc::new(VarControlBlock {
            waiting_threads: Mutex::new(Vec::new()),
            dead_threads: AtomicUsize::new(0),
            state: ParkingMutex::new(Version {
                value: Arc::new(val),
                timestamp: 0,
            }),
            owner: AtomicU64::new(0),
        })
    }

    #[cfg(not(feature = "wait-on-retry"))]
    pub fn new<T>(val: T) -> Arc<VarControlBlock>
    where
        T: Any + Sync + Send,
    {
        Arc::new(VarControlBlock {
            state: ParkingMutex::new(Version {
                value: Arc::new(val),
                timestamp: 0,
            }),
            owner: AtomicU64::new(0),
        })
    }

    pub fn get_address(&self) -> usize {
        std::ptr::from_ref::<VarControlBlock>(self) as usize
    }

    pub fn load_version(&self) -> (ArcAny, u64) {
        let guard = self.state.lock();
        (guard.value.clone(), guard.timestamp)
    }

    pub fn is_locked_by_other(&self, tx_id: u64) -> bool {
        let owner = self.owner.load(Ordering::Acquire);
        owner != 0 && owner != tx_id
    }

    pub fn try_lock_for(&self, tx_id: u64) -> bool {
        match self
            .owner
            .compare_exchange(0, tx_id, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => true,
            Err(owner) => owner == tx_id,
        }
    }

    pub fn unlock_for(&self, tx_id: u64) {
        let _ = self
            .owner
            .compare_exchange(tx_id, 0, Ordering::AcqRel, Ordering::Acquire);
    }

    #[cfg(feature = "wait-on-retry")]
    pub fn wake_all(&self) {
        let threads = {
            let mut guard = self.waiting_threads.lock();
            std::mem::take(&mut *guard)
        };

        for thread in threads.iter().filter_map(Weak::upgrade) {
            thread.set_changed();
        }
    }

    #[cfg(feature = "wait-on-retry")]
    pub fn wait(&self, thread: &Arc<ControlBlock>) {
        self.waiting_threads.lock().push(Arc::downgrade(thread));
    }

    #[cfg(feature = "wait-on-retry")]
    pub fn set_dead(&self) {
        let deads = self.dead_threads.fetch_add(1, Ordering::Relaxed);

        if deads >= 64 {
            let mut guard = self.waiting_threads.lock();
            self.dead_threads.store(0, Ordering::SeqCst);
            guard.retain(|t| t.upgrade().is_some());
        }
    }
}

impl PartialEq for VarControlBlock {
    fn eq(&self, other: &Self) -> bool {
        self.get_address() == other.get_address()
    }
}

impl Eq for VarControlBlock {}

impl Ord for VarControlBlock {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.get_address().cmp(&other.get_address())
    }
}

impl PartialOrd for VarControlBlock {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone)]
pub struct TVar<T> {
    control_block: Arc<VarControlBlock>,
    _marker: PhantomData<T>,
}

impl<T> TVar<T>
where
    T: Any + Sync + Send + Clone,
{
    pub fn new(val: T) -> TVar<T> {
        TVar {
            control_block: VarControlBlock::new(val),
            _marker: PhantomData,
        }
    }

    pub fn read_atomic(&self) -> T {
        let val = self.read_ref_atomic();

        (&*val as &dyn Any)
            .downcast_ref::<T>()
            .expect("wrong type in TVar<T>")
            .clone()
    }

    pub fn write_atomic(&self, value: T) {
        {
            let mut state = self.control_block.state.lock();
            state.timestamp = state.timestamp.saturating_add(1);
            state.value = Arc::new(value);
        }

        #[cfg(feature = "wait-on-retry")]
        self.control_block.wake_all();
    }

    pub fn read_ref_atomic(&self) -> ArcAny {
        self.control_block.load_version().0
    }

    pub fn read(&self, transaction: &mut Transaction) -> StmClosureResult<T> {
        transaction.read(self)
    }

    pub fn write(&self, transaction: &mut Transaction, value: T) -> StmClosureResult<()> {
        transaction.write(self, value)
    }

    pub fn modify<F>(&self, transaction: &mut Transaction, f: F) -> StmClosureResult<()>
    where
        F: FnOnce(T) -> T,
    {
        transaction.modify(self, f)
    }

    pub fn exchange(&self, transaction: &mut Transaction, value: T) -> StmClosureResult<T> {
        transaction.exchange(self, value)
    }

    pub fn ref_eq(this: &TVar<T>, other: &TVar<T>) -> bool {
        Arc::ptr_eq(&this.control_block, &other.control_block)
    }

    pub fn control_block(&self) -> &Arc<VarControlBlock> {
        &self.control_block
    }
}

impl<T> Debug for TVar<T>
where
    T: Any + Sync + Send + Clone,
    T: Debug,
{
    #[inline(never)]
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let x = self.read_atomic();
        f.debug_struct("TVar").field("value", &x).finish()
    }
}
