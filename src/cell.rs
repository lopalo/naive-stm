use crate::{
    private,
    transaction::{LockedTxVar, TxVar},
    MutexGuard, SharedMutex, StmVar, StmVarId,
};
use std::{
    any::Any,
    fmt,
    ops::{Deref, DerefMut},
};

type SharedValue<T> = SharedMutex<VersionedValue<T>>;

type LockedValue<'a, T> = MutexGuard<'a, VersionedValue<T>>;

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
            value: crate::shared_mutex(VersionedValue {
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
        let ver_value = self
            .value
            .lock()
            .expect("BUG: transaction commit phase must not poison a mutex");
        let initial_version = ver_value.version;
        let tx_value = ver_value.data.clone();
        drop(ver_value);
        TxCell {
            initial_version,
            value: crate::clone_shared_mutex(&self.value),
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
        Box::new(LockedTxCell {
            initial_version: *initial_version,
            value: value.lock().expect(
                "BUG: transaction commit phase must not poison a mutex",
            ),
            tx_value,
            write_tx_value: *write_tx_value,
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
    write_tx_value: bool,
}

impl<'a, T> private::Sealed for LockedTxCell<'a, T> {}

impl<'a, T> LockedTxVar for LockedTxCell<'a, T> {
    fn can_commit(&self) -> bool {
        self.initial_version == self.value.version
    }

    fn commit(&mut self) {
        if !self.write_tx_value {
            return;
        }
        self.value.version += 1;
        std::mem::swap(self.tx_value, &mut self.value.data)
    }
}
