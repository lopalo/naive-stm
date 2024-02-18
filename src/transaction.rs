use crate::{VarId, Var};
use std::{cell::RefCell, collections::BTreeMap};

//TODO: use parking_lot::Mutex;
//TODO: use rclite::Arc;

pub type Result<T = (), E = Error> = std::result::Result<T, E>;

pub struct Error;

enum TxVar {
    /// `TrackedVar` is moved into `TxRef`
    InUse,
    /// `TxRef` has been dropped, and it flushed `TrackedVar` back to a transaction
    Pending(Box<dyn TrackedVar>),
}

pub struct Tx {
    vars: RefCell<BTreeMap<VarId, TxVar>>,
}

impl Tx {
    // TODO: must return error if var is in `self.vars` and in Buffered state.
    // TODO: Use Box<Any>::downcast
    fn track<'tx, 'var: 'tx, V: Var>(
        &'tx self,
        var: &'var V,
    ) -> Result<TxRef<'tx, V::TrackedVar>> {
        todo!()
    }

    fn commit(self) -> Result {
        todo!()
    }
}

// TODO: must track original version
pub(crate) trait TrackedVar {
    fn lock(&self) -> Box<dyn TrackedVarLock>;
}

trait TrackedVarLock {
    fn can_commit(&self) -> bool;

    fn commit(&self);
}

//TODO: `T` must be concrete TrackedVar
//TODO: implement Deref(Mut)
//TODO: implement Drop that flushes TrackedVar back to the `tx` as a trait object
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
