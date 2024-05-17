use crate::{
    transaction::{LockedTxVar, TxVar},
    MutexGuard, SharedMutex, StmVar, StmVarId,
};
use std::{
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

impl<T> StmVar for StmCell<T>
where
    T: Clone,
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

impl<T> TxVar for TxCell<T> {
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
}

struct LockedTxCell<'a, T> {
    initial_version: usize,
    value: LockedValue<'a, T>,
    tx_value: &'a mut T,
    write_tx_value: bool,
}

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

#[cfg(test)]
mod test {
    use super::*;
    use crate::{Error, Tx};
    use assert_matches::assert_matches;
    use std::thread;

    fn sleep() {
        thread::sleep(std::time::Duration::from_micros(50))
    }

    fn read_cell<T: Clone>(cell: &StmCell<T>) -> T {
        Tx::run(|tx| Ok(tx.track(cell)?.clone())).unwrap()
    }

    #[test]
    fn two_transactions_add_2_cells() {
        let cell_a = StmCell::new(5);
        let cell_b = StmCell::new(17);
        thread::scope(|scope| {
            let tx_1 = scope.spawn(|| {
                Tx::run(|tx| {
                    sleep();
                    let mut a = tx.track(&cell_a)?;
                    sleep();
                    let b = tx.track(&cell_b)?;

                    assert_matches!(
                        tx.track(&cell_a),
                        Err(Error::TransactionVariableIsInUse)
                    );

                    let val = **a + **b;
                    sleep();
                    **a = val;
                    Ok(val)
                })
                .unwrap()
            });

            let tx_2 = scope.spawn(|| {
                Tx::run(|tx| {
                    sleep();
                    let b = tx.track(&cell_b)?;
                    sleep();
                    let a = tx.track(&cell_a)?;
                    sleep();
                    let val = **a + **b;
                    drop(b);
                    let mut b = tx.track(&cell_b)?;
                    **b = val;
                    sleep();
                    Ok(val)
                })
                .unwrap()
            });

            let (tx_1_val, tx_2_val) =
                (tx_1.join().unwrap(), tx_2.join().unwrap());

            let (val_a, val_b) = (read_cell(&cell_a), read_cell(&cell_b));

            assert_eq!(tx_1_val, val_a);
            assert_eq!(tx_2_val, val_b);

            // The result depends on the order in which the transactions are executed
            assert_matches!(val_a + val_b, 49 | 61);
        })
    }
}
