use std::{cell::RefCell, collections::BTreeMap};

//TODO: use parking_lot::Mutex;
//TODO: use rclite::Arc;

pub type Result<T = (), E = Error> = std::result::Result<T, E>;

pub struct Error;

//TODO: global static private atomic counter for this type;
struct TxId;

enum TrackedVar {
    Buffered,
    Flushed(Box<dyn TxVarGuard>),
}

pub struct Tx {
    tracked_vars: RefCell<BTreeMap<TxId, TrackedVar>>,
}

impl Tx {
    // TODO: must return error if var is in tracked_vars and in Buffered state.
    // TODO: Use Box<Any>::downcast
    fn track<'tx, 'var: 'tx, V: TxVar>(
        &'tx self,
        var: &'var V,
    ) -> Result<TxRef<'tx, V::Guard>> {
        todo!()
    }

    fn commit(self) -> Result {
        todo!()
    }
}

// Must be a private trait
trait TxVar {
    type Guard: TxVarGuard;

    fn tx_id(&self) -> TxId;

    // TODO: remembers the original version at this point
    fn guard(&self) -> &Self::Guard;
}

// TODO: must track original version
trait TxVarGuard {
    fn lock(&self) -> Box<dyn TxVarLock>;
}

trait TxVarLock {
    fn can_commit(&self) -> bool;

    fn commit(&self);
}

//TODO: `T` must be concrete TxVarGuard
//TODO: implement Deref(Mut)
//TODO: implement Drop that flushes TxVarGuard back to the `tx` as a trait object
struct TxRef<'tx, T: 'tx> {
    tx: &'tx Tx,
    var: Box<T>,
}

//TODO: macro track!(tx, var1, var2) => {let var1 = tx.track(var1)?; let var2 = tx.track(var2)?}

//TODO:
pub fn run_tx<T, F>(f: F) -> Result<T>
where
    F: Fn(&Tx) -> Result<T>,
{
    f(todo!())
}
