mod transaction;
mod variable;

use std::fmt;
use variable::StmVarId;

pub use transaction::Tx;
pub use variable::{cell::StmCell, map::StmMap, queue::StmQueue};

pub type Result<T = (), E = ()> = std::result::Result<T, Error<E>>;

#[derive(Debug)]
pub enum Error<E = ()> {
    TransactionVariableIsInUse(StmVarId),
    ConcurrentUpdate,
    TooManyTransactionRetryAttempts { attempts: usize },
    TransactionAbort(E),
}

impl<E> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TransactionVariableIsInUse(var_id) => write!(
                f,
                "Transaction is already tracking the STM variable `{var_id:?}`. \
                The previous `TxRef` handle for this variable must be dropped \
                before calling `Tx.track` on it again."
            ),
            Self::ConcurrentUpdate => write!(
                f,
                "Another transaction concurrently updated an STM variable. \
                Therefore, the current transaction should be retried. \
                It's a bug if this error escapes the transaction runner."
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
