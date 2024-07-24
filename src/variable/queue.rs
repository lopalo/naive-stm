use crate::{
    transaction::{LockedTxVar, TxVar},
    variable::{
        self, LockGuard, LockedVersionedValue, ReadLockedVersionedValue,
        SharedVersionedValue, StmVar, StmVarId, Version, VersionedValue,
    },
    Error, Result,
};
use std::{
    any::{self, Any},
    borrow::Cow,
    collections::VecDeque,
    fmt,
};

type SharedVersionedDeque<T> = SharedVersionedValue<VecDeque<T>>;

/// Atomic queue
#[derive(Clone)]
pub struct StmQueue<T> {
    var_id: StmVarId,
    queue: SharedVersionedDeque<T>,
}

impl<T> StmQueue<T> {
    pub fn new() -> Self {
        Self {
            var_id: StmVarId::new(),
            queue: VersionedValue::new_in_shared_lock(VecDeque::new()),
        }
    }
}

impl<T> Default for StmQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> FromIterator<T> for StmQueue<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            var_id: StmVarId::new(),
            queue: VersionedValue::new_in_shared_lock(VecDeque::from_iter(
                iter,
            )),
        }
    }
}

impl<T> StmVar for StmQueue<T>
where
    T: Clone + 'static,
{
    type TxVar = TxQueue<T>;

    fn var_id(&self) -> StmVarId {
        self.var_id
    }

    fn tx_var(&self) -> Self::TxVar {
        let initial_version = self.queue.read().version.clone();
        TxQueue {
            initial_version,
            queue: variable::clone_shared_lock(&self.queue),
            front_position: 0,
            push_back_items: VecDeque::new(),
        }
    }
}

impl<T> fmt::Debug for StmQueue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StmQueue<{}>({:?})", any::type_name::<T>(), self.var_id)
    }
}

/// A handle for [`StmQueue`] tracked by a transaction
pub struct TxQueue<T> {
    initial_version: Version,
    queue: SharedVersionedDeque<T>,
    front_position: usize,
    push_back_items: VecDeque<T>,
}

impl<T> TxQueue<T>
where
    T: Clone,
{
    /// Enqueue an element
    pub fn push(&mut self, item: T) {
        self.push_back_items.push_back(item)
    }

    /// Dequeue an element
    pub fn pop(&mut self) -> Result<Option<T>> {
        let queue = self.read_queue()?;
        let item = queue.data.get(self.front_position).cloned();
        drop(queue);
        if item.is_some() {
            self.front_position += 1;
        }
        Ok(item.or_else(|| self.push_back_items.pop_front()))
    }

    /// Get the next element to be dequeued without consuming it
    pub fn peek(&self) -> Result<Option<Cow<T>>> {
        let queue = self.read_queue()?;
        let item = queue.data.get(self.front_position).cloned().map(Cow::Owned);
        drop(queue);
        Ok(item.or_else(|| self.push_back_items.front().map(Cow::Borrowed)))
    }

    pub fn is_empty(&self) -> Result<bool> {
        if self.front_position < self.read_queue()?.data.len() {
            return Ok(false);
        }
        Ok(self.push_back_items.is_empty())
    }

    pub fn iter(&self) -> Iter<'_, T> {
        self.into_iter()
    }

    fn read_queue(&self) -> Result<ReadLockedVersionedValue<'_, VecDeque<T>>> {
        let queue = self.queue.read();
        if self.initial_version != queue.version {
            return Err(Error::ConcurrentUpdate);
        }
        Ok(queue)
    }
}

impl<T> fmt::Debug for TxQueue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TxQueue<{}>", any::type_name::<T>())
    }
}

impl<T: 'static> TxVar for TxQueue<T> {
    fn lock(&mut self) -> Box<dyn LockedTxVar + '_> {
        let Self {
            initial_version,
            queue,
            front_position,
            push_back_items,
        } = self;
        let queue = if *front_position > 0 || !push_back_items.is_empty() {
            LockGuard::Write(queue.write())
        } else {
            LockGuard::Read(queue.read())
        };
        Box::new(LockedTxQueue {
            initial_version: initial_version.clone(),
            queue,
            front_position: *front_position,
            push_back_items,
        })
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

struct LockedTxQueue<'a, T> {
    initial_version: Version,
    queue: LockedVersionedValue<'a, VecDeque<T>>,
    front_position: usize,
    push_back_items: &'a mut VecDeque<T>,
}

impl<'a, T> LockedTxVar for LockedTxQueue<'a, T> {
    fn can_commit(&self) -> bool {
        &self.initial_version == self.queue.current_version()
    }

    fn commit(&mut self) {
        let queue = match &mut self.queue {
            LockGuard::Read(_) => return,
            LockGuard::Write(queue) => queue,
        };
        queue.version.increment();
        for _ in 0..self.front_position {
            queue.data.pop_front();
        }
        queue.data.append(self.push_back_items)
    }
}

impl<'a, T> IntoIterator for &'a TxQueue<T>
where
    T: Clone,
{
    type IntoIter = Iter<'a, T>;
    type Item = <Self::IntoIter as Iterator>::Item;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            queue: self,
            cursor: 0,
        }
    }
}

pub struct Iter<'a, T> {
    queue: &'a TxQueue<T>,
    cursor: usize,
}

impl<'a, T> Iterator for Iter<'a, T>
where
    T: Clone,
{
    type Item = Result<Cow<'a, T>>;

    fn next(&mut self) -> Option<Self::Item> {
        let Self {
            queue:
                TxQueue {
                    front_position,
                    push_back_items,
                    ..
                },
            cursor,
        } = self;
        let mut position = *cursor + *front_position;
        let queue = match self.queue.read_queue() {
            Ok(queue) => queue,
            Err(err) => return Some(Err(err)),
        };
        let queue_len = queue.data.len();
        let item = queue.data.get(position).cloned().map(Cow::Owned);
        drop(queue);
        if position >= queue_len {
            position -= queue_len;
        }
        let item =
            item.or_else(|| push_back_items.get(position).map(Cow::Borrowed));
        if item.is_some() {
            *cursor += 1;
        }
        Ok(item).transpose()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn queue_operations() {
        let q = StmQueue::from_iter([10, 20, 30, 40]);
        assert!(
            format!("{q:?}").starts_with("StmQueue<i32>(StmVarId(")
        );

        crate::Tx::run(|tx| {
            crate::track! {tx, q};
            assert!(!q.is_empty()?);
            assert_eq!(q.peek()?, Some(Cow::Owned(10)));
            assert_eq!(q.pop()?, Some(10));
            assert_eq!(q.peek()?, Some(Cow::Owned(20)));
            assert_eq!(q.pop()?, Some(20));
            assert!(!q.is_empty()?);

            q.push(777);
            q.push(888);

            assert_eq!(
                q.iter().collect::<Result<Vec<_>>>()?,
                vec![
                    Cow::Owned(30),
                    Cow::Owned(40),
                    Cow::Borrowed(&777),
                    Cow::Borrowed(&888)
                ]
            );

            assert!(
                format!("{q:?}").starts_with("TxRef<TxQueue<i32>>(StmVarId(")
            );

            q.pop()?;
            q.pop()?;
            assert!(!q.is_empty()?);

            assert_eq!(q.peek()?, Some(Cow::Borrowed(&777)));
            assert_eq!(q.pop()?, Some(777));
            assert_eq!(q.peek()?, Some(Cow::Owned(888)));
            assert_eq!(q.pop()?, Some(888));
            assert!(q.is_empty()?);

            Ok(())
        })
        .unwrap()
    }
}
