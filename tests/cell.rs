use assert_matches::assert_matches;
use naive_stm::{cell::StmCell, track, Error, Tx};
use std::thread;

fn sleep() {
    thread::sleep(std::time::Duration::from_micros(50))
}

fn read_cell<T: Clone + 'static>(cell: &StmCell<T>) -> T {
    Tx::run(|tx| Ok(tx.track(cell)?.clone())).unwrap()
}

fn two_transactions_add_2_cells() {
    let cell_a = StmCell::new(5);
    let cell_b = StmCell::new(17);

    let mut tx_1_attempts = 0;
    let mut tx_2_attempts = 0;

    let (tx_1_val, tx_2_val) = thread::scope(|scope| {
        let tx_1 = scope.spawn(|| {
            Tx::run(|tx| {
                tx_1_attempts += 1;

                sleep();
                let mut a = tx.track(&cell_a)?;
                sleep();
                let b = tx.track(&cell_b)?;

                assert_matches!(
                    tx.track(&cell_a),
                    Err(Error::TransactionVariableIsInUse(_))
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
                tx_2_attempts += 1;

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
        (tx_1.join().unwrap(), tx_2.join().unwrap())
    });

    let (val_a, val_b) = (read_cell(&cell_a), read_cell(&cell_b));

    assert_eq!(tx_1_val, val_a);
    assert_eq!(tx_2_val, val_b);

    let total_tx_attempts = tx_1_attempts + tx_2_attempts;
    assert!(total_tx_attempts >= 2, "{total_tx_attempts}");

    // The result depends on the order in which the transactions are executed
    assert_matches!(val_a + val_b, 49 | 61);
}

#[test]
fn two_transactions_add_2_cells_single() {
    two_transactions_add_2_cells()
}

#[test]
fn two_transactions_add_2_cells_100_times() {
    for _ in 0..100 {
        two_transactions_add_2_cells()
    }
}

#[test]
fn triple_swap() {
    let cell_a = StmCell::new("foo");
    let cell_b = StmCell::new("bar");

    thread::scope(|scope| {
        let txs: Vec<_> = (0..3)
            .map(|_| {
                scope.spawn(|| {
                    Tx::run(|tx| {
                        track!(tx, cell_a, cell_b);
                        let ref_a: &mut &str = &mut cell_a;
                        let ref_b: &mut &str = &mut cell_b;
                        std::mem::swap(ref_a, ref_b);
                        Ok(())
                    })
                    .unwrap()
                })
            })
            .collect();
        for tx in txs {
            tx.join().unwrap();
        }
    });

    let (val_a, val_b) = (read_cell(&cell_a), read_cell(&cell_b));

    assert_eq!("bar", val_a);
    assert_eq!("foo", val_b);
}
