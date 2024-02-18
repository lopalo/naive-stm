mod cell;
mod deque;
mod map;
mod transaction;

use std::sync::atomic::{AtomicUsize, Ordering};
use transaction::TrackedVar;

struct VarId {
    id: usize,
}

impl VarId {
    fn new() -> Self {
        static CURRENT_ID: AtomicUsize = AtomicUsize::new(0);
        Self {
            id: CURRENT_ID.fetch_add(1, Ordering::SeqCst),
        }
    }
}

// Must be a private trait
trait Var {
    type TrackedVar: TrackedVar;

    fn var_id(&self) -> VarId;

    // TODO: remembers the original version at this point
    fn tracked_var(&self) -> &Self::TrackedVar;
}
