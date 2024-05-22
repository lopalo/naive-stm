pub mod cell;
pub mod deque;
pub mod map;
mod transaction;

use std::{
    fmt,
    sync::atomic::{AtomicUsize, Ordering},
};
use transaction::TxVar;

pub use transaction::Tx;

pub type Result<T = (), E = ()> = std::result::Result<T, Error<E>>;

#[derive(Debug)]
pub enum Error<E = ()> {
    TransactionVariableIsInUse(StmVarId),
    TooManyTransactionRetryAttempts { attempts: usize },
    TransactionAbort(E),
}

impl<E> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TransactionVariableIsInUse(var_id) => write!(
                f,
                "Transaction is already tracking the variable `{var_id:?}`. \
                The previous `TxRef` handle for this variable must be dropped \
                before calling `Tx.track` on it again."
            ),
            Self::TooManyTransactionRetryAttempts { attempts } => {
                write!(f, "The maximum number ({attempts}) of attempts for the transaction has been reached")
            }
            Self::TransactionAbort(_) => {
                write!(f, "Transaction was explicitly aborted")
            }
        }
    }
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

type SharedRwLock<T> = rclite::Arc<parking_lot::RwLock<T>>;

fn shared_lock<T>(value: T) -> SharedRwLock<T> {
    rclite::Arc::new(parking_lot::RwLock::new(value))
}

fn clone_shared_lock<T>(lock: &SharedRwLock<T>) -> SharedRwLock<T> {
    rclite::Arc::clone(lock)
}

enum LockGuard<'a, T> {
    Read(parking_lot::RwLockReadGuard<'a, T>),
    Write(parking_lot::RwLockWriteGuard<'a, T>),
}

mod private {
    pub trait Sealed {}
}
