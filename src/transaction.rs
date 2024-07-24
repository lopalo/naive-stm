use crate::{variable::StmVar, Error, Result, StmVarId};
use rand::prelude::*;
use std::{
    any::Any,
    cell::RefCell,
    collections::{btree_map::Entry, BTreeMap},
    fmt,
    ops::{Deref, DerefMut},
    thread,
    time::Duration,
};

/// Options to run a transaction with
pub struct TxOptions {
    /// How many times a transaction will be retried in case of concurrent updates
    pub attempts: usize,
    /// A pause before the next attempt to complete a transaction
    pub retry_pause: Duration,
    /// If `true`, the pause between transaction attempts will be random
    /// value withing the range `0 .. retry_pause`
    pub pause_jitter: bool,
}

impl Default for TxOptions {
    fn default() -> Self {
        Self {
            attempts: 10,
            retry_pause: Duration::ZERO,
            pause_jitter: false,
        }
    }
}

enum TrackedVar {
    /// [`TxVar`] is moved into [`TxRef`]
    InUse,
    /// [`TxRef`] has been dropped, and it flushed [`TxVar`] back to a transaction
    Pending(Box<dyn TxVar>),
}

/// Transaction executor
pub struct Tx {
    vars: RefCell<BTreeMap<StmVarId, TrackedVar>>,
}

enum CommitStatus {
    Success,
    Fail,
}

impl Tx {
    /// Run a new transaction which will be automatically retried in case of concurrent updates
    pub fn run<F, T, E>(f: F) -> Result<T, E>
    where
        F: FnMut(&Tx) -> Result<T, E>,
    {
        Self::run_with_options(&Default::default(), f)
    }

    /// Like [`run`](#method.run) but with non-default options
    pub fn run_with_options<F, T, E>(
        options: &TxOptions,
        mut f: F,
    ) -> Result<T, E>
    where
        F: FnMut(&Tx) -> Result<T, E>,
    {
        let TxOptions {
            attempts,
            retry_pause,
            pause_jitter,
        } = *options;
        let mut rng = rand::thread_rng();

        for attempt in 0..attempts {
            if attempt > 0 {
                let mut pause = retry_pause;
                if pause_jitter {
                    pause = pause.mul_f32(rng.gen())
                }
                thread::sleep(pause);
            }

            let tx = Self {
                vars: RefCell::new(BTreeMap::new()),
            };
            let result = f(&tx);
            if let Err(Error::ConcurrentUpdate) = result {
                continue;
            }
            let output = result?;
            match tx.commit() {
                CommitStatus::Success => return Ok(output),
                CommitStatus::Fail => (),
            }
        }

        Err(Error::TooManyTransactionRetryAttempts { attempts })
    }

    /// Make the transaction track an STM variable for changes made within the current
    /// transaction and for changes made by concurrently commited transactions.
    ///
    /// The method returns a handle that allows isolated read/write operations on the variable.
    /// All the changes made to the same STM variable withing the same transaction are preserved
    /// between the calls of `Tx::track`.
    ///
    /// Returns an error if there is another alive handle for the variable in the current transaction.
    pub fn track<'tx, V: StmVar>(
        &'tx self,
        var: &V,
    ) -> Result<TxRef<'tx, V::TxVar>> {
        let var_id = var.var_id();
        let tx_var = match self.vars.borrow_mut().entry(var_id) {
            Entry::Vacant(entry) => {
                entry.insert(TrackedVar::InUse);
                Box::new(var.tx_var())
            }
            Entry::Occupied(mut entry) => {
                match std::mem::replace(entry.get_mut(), TrackedVar::InUse) {
                    TrackedVar::InUse => {
                        return Err(Error::TransactionVariableIsInUse(var_id))
                    }
                    TrackedVar::Pending(tx_var) => tx_var
                        .into_any()
                        .downcast()
                        .expect(
                        "BUG: variable type must be uniquely identified by its ID",
                    ),
                }
            }
        };
        Ok(TxRef {
            tx: self,
            var_id,
            var: Some(tx_var),
        })
    }

    fn commit(mut self) -> CommitStatus {
        // The variables will be locked in the ascending order of their IDs.
        let locked_vars: Vec<_> = self
            .vars
            .get_mut()
            .values_mut()
            .map(|tracked_var| {
                let TrackedVar::Pending(tx_var) = tracked_var else {
                    panic!("BUG: there must be no `TxRef` around for this transaction");
                };
                tx_var.lock()
            })
            .collect();
        for var in &locked_vars {
            if !var.can_commit() {
                return CommitStatus::Fail;
            }
        }
        for mut var in locked_vars {
            var.commit()
        }
        CommitStatus::Success
    }

    /// Abort current transaction and prevent it from futher retrying
    pub fn abort() -> Result<(), ()> {
        Self::abort_with(())
    }

    /// Aborth transaction with the given error.
    /// This method should be used for propagating custom errors out of a transaction.
    pub fn abort_with<E>(error: E) -> Result<(), E> {
        Err(Error::TransactionAbort(error))
    }
}

/// Implementors must track the original version of variable's value
pub trait TxVar: 'static {
    /// This method is called in the commit phase of a transaction.
    /// [`LockedTxVar`] is responsible for checking whether the variable's value
    /// has changed while the transaction was running.
    fn lock(&mut self) -> Box<dyn LockedTxVar + '_>;

    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

pub trait LockedTxVar {
    /// Checks if the variable's value has changed since the first read
    fn can_commit(&self) -> bool;

    /// Writes data generated by a transaction to a shared transaction variable,
    /// thus making the changes visible to other transactions.
    fn commit(&mut self);
}

/// A wrapper for an STM variable that is tracked by a transaction.
pub struct TxRef<'tx, T>
where
    // Unfortunately, `Drop` impl requires this bound on the struct
    T: TxVar,
{
    tx: &'tx Tx,
    var_id: StmVarId,
    var: Option<Box<T>>,
}

static NO_VAR_ERROR_MSG: &str =
    "BUG: transaction variable must be present for entire lifetime";

impl<T: TxVar> TxRef<'_, T> {
    fn get_var(&self) -> &T {
        self.var.as_deref().expect(NO_VAR_ERROR_MSG)
    }

    fn get_var_mut(&mut self) -> &mut T {
        self.var.as_deref_mut().expect(NO_VAR_ERROR_MSG)
    }
}

impl<'tx, T: TxVar> Drop for TxRef<'tx, T> {
    fn drop(&mut self) {
        let Self { tx, var_id, var } = self;
        let tx_var_status = tx.vars.borrow_mut().insert(
            *var_id,
            TrackedVar::Pending(var.take().expect(NO_VAR_ERROR_MSG)),
        );
        let Some(TrackedVar::InUse) = tx_var_status else {
            panic!("BUG: transaction has not been tracking the variable `{var_id:?}`")
        };
    }
}

impl<T: TxVar> Deref for TxRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.get_var()
    }
}

impl<T: TxVar> DerefMut for TxRef<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.get_var_mut()
    }
}

impl<T: TxVar + fmt::Debug> fmt::Debug for TxRef<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TxRef<{:?}>({:?})", self.get_var(), self.var_id)
    }
}

/// A helper to create tracked transaction variables from STM variables in the current block scope.
///
/// # Examples
///
/// ```
/// use naive_stm::{Tx, StmCell, StmQueue, track};
///
/// let cell = StmCell::new(777);
/// let queue = StmQueue::from_iter([23]);
/// Tx::run(|tx| {
///     track!(tx, cell, queue);
///     *cell.get_mut() = queue.pop()?.unwrap();
///     assert_eq!(23, *cell.get());
///     Ok(())
/// });
///
/// ```
#[macro_export]
macro_rules! track {
    ($tx:ident, $($stm_var:ident),+) => {
        $(
            #[allow(unused_mut)]
            let mut $stm_var = $tx.track(&$stm_var)?;
        )+
    };
}
