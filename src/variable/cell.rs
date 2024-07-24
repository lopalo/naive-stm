use crate::{
    transaction::{LockedTxVar, TxVar},
    variable::{
        self, LockGuard, LockedVersionedValue, SharedVersionedValue, StmVar,
        StmVarId, Version, VersionedValue,
    },
};
use std::{
    any::{self, Any},
    fmt,
    ops::{Deref, DerefMut},
};

/// Atomic single element container
#[derive(Clone)]
pub struct StmCell<T> {
    var_id: StmVarId,
    value: SharedVersionedValue<T>,
}

impl<T> StmCell<T> {
    pub fn new(value: T) -> Self {
        Self {
            var_id: StmVarId::new(),
            value: VersionedValue::new_in_shared_lock(value),
        }
    }
}

impl<T> StmVar for StmCell<T>
where
    T: Clone + 'static,
{
    type TxVar = TxCell<T>;

    fn var_id(&self) -> StmVarId {
        self.var_id
    }

    fn tx_var(&self) -> Self::TxVar {
        let ver_value = self.value.read();
        let initial_version = ver_value.version.clone();
        let tx_value = ver_value.data.clone();
        drop(ver_value);
        TxCell {
            initial_version,
            value: variable::clone_shared_lock(&self.value),
            tx_value,
            write_tx_value: false,
        }
    }
}

impl<T> fmt::Debug for StmCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StmCell<{}>({:?})", any::type_name::<T>(), self.var_id)
    }
}

/// A handle for [`StmCell`] tracked by a transaction
pub struct TxCell<T> {
    initial_version: Version,
    value: SharedVersionedValue<T>,
    tx_value: T,
    write_tx_value: bool,
}

impl<T> TxCell<T> {
    /// Reference to the in-transaction value of the cell
    pub fn get(&self) -> &T {
        &self.tx_value
    }

    /// Mutable reference to the in-transaction value of the cell
    pub fn get_mut(&mut self) -> &mut T {
        self.write_tx_value = true;
        &mut self.tx_value
    }

    /// Takes the value out of the cell, leaving the default value of `T`
    pub fn take(&mut self) -> T
    where
        T: Default,
    {
        std::mem::take(self.get_mut())
    }
}

impl<T> Deref for TxCell<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.get()
    }
}

impl<T> DerefMut for TxCell<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.get_mut()
    }
}

impl<T> fmt::Debug for TxCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TxCell<{}>", any::type_name::<T>())
    }
}

impl<T: 'static> TxVar for TxCell<T> {
    fn lock(&mut self) -> Box<dyn LockedTxVar + '_> {
        let Self {
            initial_version,
            value,
            tx_value,
            write_tx_value,
        } = self;
        let value = if *write_tx_value {
            LockGuard::Write(value.write())
        } else {
            LockGuard::Read(value.read())
        };
        Box::new(LockedTxCell {
            initial_version: initial_version.clone(),
            value,
            tx_value,
        })
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

struct LockedTxCell<'a, T> {
    initial_version: Version,
    value: LockedVersionedValue<'a, T>,
    tx_value: &'a mut T,
}

impl<'a, T> LockedTxVar for LockedTxCell<'a, T> {
    fn can_commit(&self) -> bool {
        &self.initial_version == self.value.current_version()
    }

    fn commit(&mut self) {
        let value = match &mut self.value {
            LockGuard::Read(_) => return,
            LockGuard::Write(value) => value,
        };
        value.version.increment();
        std::mem::swap(self.tx_value, &mut value.data)
    }
}
