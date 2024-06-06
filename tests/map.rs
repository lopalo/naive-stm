#![allow(clippy::disallowed_names)]

use naive_stm::{
    track, Result, Tx, {StmMap, TxMap},
};
use std::{collections::BTreeMap, thread, time::Duration};

fn drain_map<K, V>(map: &StmMap<K, V>) -> BTreeMap<K, V>
where
    K: Ord + Clone + 'static,
    V: Clone + 'static,
{
    Tx::run(|tx| {
        track! {tx, map};
        let mut res = BTreeMap::new();
        while let Some(key) = map.first_key()? {
            let key = key.into_owned();
            let val = map.get(&key)?.unwrap().into_owned();
            res.insert(key.clone(), val);
            map.remove(key);
        }
        Ok(res)
    })
    .unwrap()
}

fn make_counters<I>(keys: I) -> StmMap<I::Item, usize>
where
    I: IntoIterator<Item = &'static str>,
{
    keys.into_iter()
        .chain(["removed_items"])
        .map(|k| (k, 0))
        .collect()
}

fn move_item(
    key: &'static str,
    from: &mut TxMap<&'static str, usize>,
    to: &mut TxMap<&'static str, usize>,
) -> Result {
    if let Some(&item) = from.get(key)?.as_deref() {
        from.remove(key);
        to.insert(key, item + 1);
        *from.get_mut("removed_items")?.unwrap() += 1;
    }
    thread::sleep(Duration::from_millis(1));
    Ok(())
}

#[test]
fn maps_grouping() {
    let foo_keys = ["b2", "d9", "a1", "c0", "b5", "b7"];
    let bar_keys = ["d7", "a9", "c2", "c1", "d5", "c7", "d8"];
    let baz_keys = ["c6", "b9", "a2", "c3", "a4", "a7"];
    let qux_keys = ["a0", "b0", "d2"];
    let all_keys: Vec<_> = foo_keys
        .into_iter()
        .chain(bar_keys)
        .chain(baz_keys)
        .chain(qux_keys)
        .collect();
    let foo = &make_counters(foo_keys);
    let bar = &make_counters(bar_keys);
    let baz = &make_counters(baz_keys);
    let qux = &make_counters(qux_keys);
    let buckets = [foo, bar, baz, qux];
    thread::scope(|scope| {
        scope.spawn(|| {
            Tx::run(|tx| {
                for bucket in buckets {
                    if bucket == baz {
                        continue;
                    }
                    track! {tx, bucket, baz};
                    for key in &all_keys {
                        if key.starts_with('c') {
                            move_item(key, &mut bucket, &mut baz)?;
                        }
                    }
                }
                Ok(())
            })
            .unwrap()
        });
        scope.spawn(|| {
            Tx::run(|tx| {
                for bucket in buckets {
                    for key in &all_keys {
                        if bucket != bar && key.starts_with('b') {
                            track! {tx, bucket, bar};
                            move_item(key, &mut bucket, &mut bar)?;
                        }
                        if bucket != qux && key.starts_with('d') {
                            track! {tx, bucket, qux};
                            move_item(key, &mut bucket, &mut qux)?;
                        }
                    }
                }
                Ok(())
            })
            .unwrap()
        });
        scope.spawn(|| {
            Tx::run(|tx| {
                for bucket in buckets {
                    if bucket == foo {
                        continue;
                    }
                    track! {tx, bucket, foo};
                    for key in &all_keys {
                        if key.starts_with('a') {
                            move_item(key, &mut bucket, &mut foo)?;
                        }
                    }
                }
                Ok(())
            })
            .unwrap()
        });
    });

    assert_eq!(
        drain_map(foo),
        [
            ("a0", 1),
            ("a1", 0),
            ("a2", 1),
            ("a4", 1),
            ("a7", 1),
            ("a9", 1),
            ("removed_items", 5),
        ]
        .into()
    );
    assert_eq!(
        drain_map(bar),
        [
            ("b0", 1),
            ("b2", 1),
            ("b5", 1),
            ("b7", 1),
            ("b9", 1),
            ("removed_items", 7),
        ]
        .into()
    );
    assert_eq!(
        drain_map(baz),
        [
            ("c0", 1),
            ("c1", 1),
            ("c2", 1),
            ("c3", 0),
            ("c6", 0),
            ("c7", 1),
            ("removed_items", 4),
        ]
        .into()
    );
    assert_eq!(
        drain_map(qux),
        [
            ("d2", 0),
            ("d5", 1),
            ("d7", 1),
            ("d8", 1),
            ("d9", 1),
            ("removed_items", 2),
        ]
        .into()
    );
}
