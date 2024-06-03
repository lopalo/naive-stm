pub mod cell;
pub mod map;
pub mod queue;

use crate::transaction::TxVar;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct StmVarId(usize);

impl StmVarId {
    fn new() -> Self {
        static CURRENT_ID: AtomicUsize = AtomicUsize::new(0);
        Self(CURRENT_ID.fetch_add(1, Ordering::SeqCst))
    }
}

/// A variable to be shared across multiple transactions.
///
/// The trait is private since it's not re-exported in the root module.
pub trait StmVar {
    type TxVar: TxVar;

    fn var_id(&self) -> StmVarId;

    /// Implementation must remember the original version of a variable at this point
    fn tx_var(&self) -> Self::TxVar;
}

impl<T> StmVar for &T
where
    T: StmVar,
{
    type TxVar = T::TxVar;

    fn var_id(&self) -> StmVarId {
        T::var_id(self)
    }

    fn tx_var(&self) -> Self::TxVar {
        T::tx_var(self)
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Version(usize);

impl Version {
    fn new() -> Self {
        Self(0)
    }

    fn increment(&mut self) {
        self.0 += 1
    }
}

struct VersionedValue<T> {
    version: Version,
    data: T,
}

impl<T> VersionedValue<T> {
    fn new_in_shared_lock(data: T) -> SharedVersionedValue<T> {
        rclite::Arc::new(parking_lot::RwLock::new(Self {
            version: Version::new(),
            data,
        }))
    }
}

enum LockGuard<'a, T> {
    Read(parking_lot::RwLockReadGuard<'a, T>),
    Write(parking_lot::RwLockWriteGuard<'a, T>),
}

impl<'a, T> LockGuard<'a, VersionedValue<T>> {
    fn current_version(&self) -> &Version {
        match &self {
            LockGuard::Read(queue) => &queue.version,
            LockGuard::Write(queue) => &queue.version,
        }
    }
}

type SharedRwLock<T> = rclite::Arc<parking_lot::RwLock<T>>;

type SharedVersionedValue<T> = SharedRwLock<VersionedValue<T>>;

type LockedVersionedValue<'a, T> = LockGuard<'a, VersionedValue<T>>;

type ReadLockedVersionedValue<'a, T> =
    parking_lot::RwLockReadGuard<'a, VersionedValue<T>>;

fn clone_shared_lock<T>(lock: &SharedRwLock<T>) -> SharedRwLock<T> {
    rclite::Arc::clone(lock)
}

macro_rules! impl_stm_var_eq {
    ($($stm_var_ty:ident<$($ty_param:ident),*>),*)  => {$(
        impl <$($ty_param),*> PartialEq for $stm_var_ty <$($ty_param),*>
        where Self: StmVar
        {
            fn eq(&self, other: &Self) -> bool {
                self.var_id() == other.var_id()
            }
        }
    )*}
}
use crate::{StmCell, StmMap, StmQueue};
impl_stm_var_eq! {StmCell<T>, StmQueue<T>, StmMap<K, V>}
