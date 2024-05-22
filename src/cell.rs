use crate::{
    private,
    transaction::{LockedTxVar, TxVar},
    LockGuard, SharedRwLock, StmVar, StmVarId,
};
use std::{
    any::Any,
    fmt,
    ops::{Deref, DerefMut},
};

type SharedValue<T> = SharedRwLock<VersionedValue<T>>;

type LockedValue<'a, T> = LockGuard<'a, VersionedValue<T>>;

#[derive(Clone)]
pub struct StmCell<T> {
    var_id: StmVarId,
    value: SharedValue<T>,
}

struct VersionedValue<T> {
    version: usize,
    data: T,
}

impl<T> StmCell<T> {
    pub fn new(value: T) -> Self {
        Self {
            var_id: StmVarId::new(),
            value: crate::shared_lock(VersionedValue {
                version: 0,
                data: value,
            }),
        }
    }
}

impl<T> private::Sealed for StmCell<T> {}

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
        let initial_version = ver_value.version;
        let tx_value = ver_value.data.clone();
        drop(ver_value);
        TxCell {
            initial_version,
            value: crate::clone_shared_lock(&self.value),
            tx_value,
            write_tx_value: false,
        }
    }
}

pub struct TxCell<T> {
    initial_version: usize,
    value: SharedValue<T>,
    tx_value: T,
    write_tx_value: bool,
}

impl<T> TxCell<T> {
    pub fn get(&self) -> &T {
        &self.tx_value
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.write_tx_value = true;
        &mut self.tx_value
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

impl<T: fmt::Debug> fmt::Debug for TxCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TxCell({:?})", self.tx_value)
    }
}

impl<T> private::Sealed for TxCell<T> {}

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
            initial_version: *initial_version,
            value,
            tx_value,
        })
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

struct LockedTxCell<'a, T> {
    initial_version: usize,
    value: LockedValue<'a, T>,
    tx_value: &'a mut T,
}

impl<'a, T> private::Sealed for LockedTxCell<'a, T> {}

impl<'a, T> LockedTxVar for LockedTxCell<'a, T> {
    fn can_commit(&self) -> bool {
        let current_version = match &self.value {
            LockGuard::Read(value) => value.version,
            LockGuard::Write(value) => value.version,
        };
        self.initial_version == current_version
    }

    fn commit(&mut self) {
        let value = match &mut self.value {
            LockGuard::Read(_) => return,
            LockGuard::Write(value) => value,
        };
        value.version += 1;
        std::mem::swap(self.tx_value, &mut value.data)
    }
}
