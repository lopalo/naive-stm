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
    borrow::{Borrow, Cow},
    collections::{BTreeMap, BTreeSet},
    fmt,
    ops::Bound,
};

type SharedVersionedMap<K, V> = SharedVersionedValue<BTreeMap<K, V>>;

/// Atomic map sorted by key
#[derive(Clone)]
pub struct StmMap<K, V> {
    var_id: StmVarId,
    map: SharedVersionedMap<K, V>,
}

impl<K, V> StmMap<K, V> {
    pub fn new() -> Self {
        Self {
            var_id: StmVarId::new(),
            map: VersionedValue::new_in_shared_lock(BTreeMap::new()),
        }
    }
}

impl<K, V> Default for StmMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> FromIterator<(K, V)> for StmMap<K, V>
where
    K: Ord,
{
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Self {
            var_id: StmVarId::new(),
            map: VersionedValue::new_in_shared_lock(BTreeMap::from_iter(iter)),
        }
    }
}

impl<K, V> StmVar for StmMap<K, V>
where
    K: Ord + 'static,
    V: Clone + 'static,
{
    type TxVar = TxMap<K, V>;

    fn var_id(&self) -> StmVarId {
        self.var_id
    }

    fn tx_var(&self) -> Self::TxVar {
        let initial_version = self.map.read().version.clone();
        TxMap {
            initial_version,
            map: variable::clone_shared_lock(&self.map),
            tx_map: BTreeMap::new(),
            tx_removed_keys: BTreeSet::new(),
        }
    }
}

impl<K, V> fmt::Debug for StmMap<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let key_type = any::type_name::<K>();
        let value_type = any::type_name::<V>();
        write!(f, "StmMap<{key_type}, {value_type}>({:?})", self.var_id)
    }
}

/// A handle for [`StmMap`] tracked by a transaction
pub struct TxMap<K, V> {
    initial_version: Version,
    map: SharedVersionedMap<K, V>,
    tx_map: BTreeMap<K, V>,
    tx_removed_keys: BTreeSet<K>,
}

impl<K, V> TxMap<K, V>
where
    K: Ord,
    V: Clone,
{
    pub fn insert(&mut self, key: K, value: V) {
        self.tx_removed_keys.remove(&key);
        self.tx_map.insert(key, value);
    }

    pub fn get<Q>(&self, key: &Q) -> Result<Option<Cow<V>>>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        if let Some(value) = self.tx_map.get(key) {
            return Ok(Some(Cow::Borrowed(value)));
        }
        if self.tx_removed_keys.contains(key) {
            return Ok(None);
        }
        Ok(self.read_map()?.data.get(key).cloned().map(Cow::Owned))
    }

    pub fn get_mut<Q>(&mut self, key: &Q) -> Result<Option<&mut V>>
    where
        K: Borrow<Q> + Clone,
        Q: Ord + ?Sized,
    {
        if self.tx_map.contains_key(key) {
            return Ok(self.tx_map.get_mut(key));
        }
        if self.tx_removed_keys.contains(key) {
            return Ok(None);
        }
        let map = self.read_map()?;
        let key_value = map.data.get_key_value(key);
        if let Some((key, value)) = key_value {
            let (key, value) = (key.clone(), value.clone());
            drop(map);
            return Ok(Some(self.tx_map.entry(key).or_insert(value)));
        }
        Ok(None)
    }

    pub fn contains_key<Q>(&self, key: &Q) -> Result<bool>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        if self.tx_map.contains_key(key) {
            return Ok(true);
        }
        if self.tx_removed_keys.contains(key) {
            return Ok(false);
        }
        Ok(self.read_map()?.data.contains_key(key))
    }

    /// Returns the minimum key in the map. If result is `None`, then the map is empty.
    pub fn first_key(&self) -> Result<Option<Cow<K>>>
    where
        K: Clone,
    {
        let Self {
            tx_map,
            tx_removed_keys,
            ..
        } = self;
        let map = self.read_map()?;
        let map_min_key = map
            .data
            .keys()
            .find(|key| !tx_removed_keys.contains(key))
            .cloned()
            .map(Cow::<'_, K>::Owned);
        drop(map);
        let tx_map_min_key = tx_map.keys().next().map(Cow::Borrowed);
        Ok(match (map_min_key, tx_map_min_key) {
            (Some(map_min_key), Some(tx_map_min_key)) => {
                Some(map_min_key.min(tx_map_min_key))
            }
            (Some(map_min_key), None) => Some(map_min_key),
            (None, Some(tx_map_min_key)) => Some(tx_map_min_key),
            (None, None) => None,
        })
    }

    pub fn remove(&mut self, key: K) {
        self.tx_map.remove(&key);
        self.tx_removed_keys.insert(key);
    }

    pub fn iter(&self) -> Iter<'_, K, V>
    where
        K: Clone,
    {
        self.into_iter()
    }

    fn read_map(&self) -> Result<ReadLockedVersionedValue<'_, BTreeMap<K, V>>> {
        let map = self.map.read();
        if self.initial_version != map.version {
            return Err(Error::ConcurrentUpdate);
        }
        Ok(map)
    }
}

impl<K, V> fmt::Debug for TxMap<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let key_type = any::type_name::<K>();
        let value_type = any::type_name::<V>();
        write!(f, "TxMap<{key_type}, {value_type}>")
    }
}

impl<K, V> TxVar for TxMap<K, V>
where
    K: Ord + 'static,
    V: 'static,
{
    fn lock(&mut self) -> Box<dyn LockedTxVar + '_> {
        let Self {
            initial_version,
            map,
            tx_map,
            tx_removed_keys,
        } = self;
        let map = if tx_map.is_empty() && tx_removed_keys.is_empty() {
            LockGuard::Read(map.read())
        } else {
            LockGuard::Write(map.write())
        };
        Box::new(LockedTxMap {
            initial_version: initial_version.clone(),
            map,
            tx_map,
            tx_removed_keys,
        })
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

struct LockedTxMap<'a, K, V> {
    initial_version: Version,
    map: LockedVersionedValue<'a, BTreeMap<K, V>>,
    tx_map: &'a mut BTreeMap<K, V>,
    tx_removed_keys: &'a mut BTreeSet<K>,
}

impl<'a, K, V> LockedTxVar for LockedTxMap<'a, K, V>
where
    K: Ord,
{
    fn can_commit(&self) -> bool {
        &self.initial_version == self.map.current_version()
    }

    fn commit(&mut self) {
        let map = match &mut self.map {
            LockGuard::Read(_) => return,
            LockGuard::Write(map) => map,
        };
        map.version.increment();
        for k in self.tx_removed_keys.iter() {
            map.data.remove(k);
        }
        map.data.append(self.tx_map)
    }
}

impl<'a, K, V> IntoIterator for &'a TxMap<K, V>
where
    K: Ord + Clone,
    V: Clone,
{
    type IntoIter = Iter<'a, K, V>;
    type Item = <Self::IntoIter as Iterator>::Item;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            map: self,
            cursor: Bound::Unbounded,
        }
    }
}

pub struct Iter<'a, K, V> {
    map: &'a TxMap<K, V>,
    cursor: Bound<K>,
}

impl<'a, K, V> Iterator for Iter<'a, K, V>
where
    K: Ord + Clone,
    V: Clone,
{
    type Item = Result<(K, V)>;

    fn next(&mut self) -> Option<Self::Item> {
        let Self {
            map:
                TxMap {
                    tx_map,
                    tx_removed_keys,
                    ..
                },
            cursor,
        } = self;
        let range = (cursor.as_ref(), Bound::Unbounded.as_ref());
        let map = match self.map.read_map() {
            Ok(map) => map,
            Err(err) => return Some(Err(err)),
        };
        let map_min_key_val = map
            .data
            .range(range)
            .find(|key_val| !tx_removed_keys.contains(key_val.0));
        let tx_map_min_key_val = tx_map.range(range).next();
        let min_key_val = match (map_min_key_val, tx_map_min_key_val) {
            (Some(map_min_key_val), Some(tx_map_min_key_val)) => {
                Some(if map_min_key_val.0 < tx_map_min_key_val.0 {
                    map_min_key_val
                } else {
                    drop(map);
                    tx_map_min_key_val
                })
            }
            (Some(map_min_key_val), None) => Some(map_min_key_val),
            (None, Some(tx_map_min_key_val)) => {
                drop(map);
                Some(tx_map_min_key_val)
            }
            (None, None) => None,
        }
        .map(owned_key_value);
        if let Some(ref min_key_val) = min_key_val {
            *cursor = Bound::Excluded(min_key_val.0.clone())
        }
        Ok(min_key_val).transpose()
    }
}

fn owned_key_value<K, V>(key_val: (&K, &V)) -> (K, V)
where
    K: Clone,
    V: Clone,
{
    let (key, val) = key_val;
    (key.clone(), val.clone())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn map_operations() {
        let m = StmMap::from_iter([
            (30, 303),
            (20, 202),
            (10, 101),
            (40, 404),
            (50, 505),
        ]);

        assert!(
            format!("{m:?}").starts_with("StmMap<i32, i32>(StmVarId(")
        );

        crate::Tx::run(|tx| {
            crate::track! {tx, m};
            assert_eq!(m.first_key()?, Some(Cow::Owned(10)));

            m.remove(30);
            m.insert(25, 2525);
            m.insert(10, 8888);
            m.remove(40);
            m.insert(30, 9999);

            assert!(m.contains_key(&20)?);
            assert_eq!(m.get(&20)?, Some(Cow::Owned(202)));
            assert!(m.contains_key(&10)?);
            assert_eq!(m.get(&10)?, Some(Cow::Owned(8888)));
            assert!(!m.contains_key(&40)?);
            assert_eq!(m.get(&40)?, None);
            assert_eq!(m.get_mut(&40)?, None);

            assert_eq!(
                m.iter().collect::<Result<Vec<_>>>()?,
                vec![(10, 8888), (20, 202), (25, 2525), (30, 9999), (50, 505)]
            );

            assert!(format!("{m:?}")
                .starts_with("TxRef<TxMap<i32, i32>>(StmVarId("));

            assert_eq!(m.first_key()?, Some(Cow::Owned(10)));
            m.remove(10);
            assert_eq!(m.first_key()?, Some(Cow::Owned(20)));
            m.remove(20);
            assert_eq!(m.first_key()?, Some(Cow::Owned(25)));
            m.remove(30);
            m.remove(25);
            assert_eq!(m.first_key()?, Some(Cow::Owned(50)));
            m.remove(50);
            assert_eq!(m.first_key()?, None);

            Ok(())
        })
        .unwrap()
    }
}
