pub mod cell;
pub mod deque;
pub mod map;
mod transaction;

use std::sync::atomic::{AtomicUsize, Ordering};
use transaction::TxVar;

pub use transaction::Tx;

pub type Result<T = (), E = ()> = std::result::Result<T, Error<E>>;

#[derive(Debug)]
pub enum Error<E = ()> {
    TransactionVariableIsInUse,
    TooManyTransactionRetryAttempts,
    TransactionAbort(E),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct StmVarId {
    id: usize,
}

impl StmVarId {
    fn new() -> Self {
        static CURRENT_ID: AtomicUsize = AtomicUsize::new(0);
        Self {
            id: CURRENT_ID.fetch_add(1, Ordering::SeqCst),
        }
    }
}

/// A variable to be shared across multiple transactions
pub trait StmVar: private::Sealed {
    type TxVar: TxVar;

    fn var_id(&self) -> StmVarId;

    /// Implementation must remember the original version of a variable at this point
    fn tx_var(&self) -> Self::TxVar;
}

//TODO: use parking_lot::Mutex or RwLock;
//TODO: use rclite::Arc;
type SharedMutex<T> = std::sync::Arc<std::sync::Mutex<T>>;

fn shared_mutex<T>(value: T) -> SharedMutex<T> {
    std::sync::Arc::new(std::sync::Mutex::new(value))
}

fn clone_shared_mutex<T>(mutex: &SharedMutex<T>) -> SharedMutex<T> {
    std::sync::Arc::clone(mutex)
}

type MutexGuard<'a, T> = std::sync::MutexGuard<'a, T>;

mod private {
    pub trait Sealed {}
}
