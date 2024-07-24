use naive_stm::{
    track, Error, Result, StmCell, StmMap, StmQueue, Tx, TxOptions,
};
use rand::{seq::SliceRandom, Rng};
use std::{
    thread,
    time::{Duration, Instant},
};

#[test]
fn mixed_containers() {
    let total_fuel = 830_029_usize;
    let fuel_per_step = 777_usize;
    let keys = ["b2", "d9", "a1", "a2", "c0", "b5", "b7", "c6"];
    let tx_opts = TxOptions {
        attempts: 20,
        retry_pause: Duration::from_micros(100),
        pause_jitter: true,
    };

    // Each worker will pass some amount of fuel from `source` to next containers
    // and then eventually to the `sink`. The workers will stop when all the fuel
    // has been transferred into the `sink`.

    let source = StmCell::new(total_fuel);
    let sink = StmCell::new(0);

    let queue = StmQueue::<(String, usize)>::new();
    let map = StmMap::<String, StmCell<usize>>::new();

    let worker_timeout = Duration::from_secs(5);
    thread::scope(|scope| {
        let workers: Vec<_> = (0..24)
            .map(|_| {
                scope.spawn(|| {
                    let start_time = Instant::now();
                    let mut rng = rand::thread_rng();
                    loop {
                        Tx::run_with_options(&tx_opts, |tx| {
                            track!(tx, source, queue);
                            let fuel =
                                rng.gen_range(0..fuel_per_step).min(**source);
                            if fuel == 0 {
                                return Ok(());
                            }
                            **source -= fuel;
                            let key = keys.choose(&mut rng).unwrap();
                            queue.push(((*key).to_owned(), fuel));
                            Ok(())
                        })
                        .or_else(ignore_too_many_attempts)
                        .unwrap();
                        Tx::run_with_options(&tx_opts, |tx| {
                            track!(tx, queue, map);
                            while let Some((key, fuel)) = queue.pop()? {
                                if !map.contains_key(&key)? {
                                    map.insert(key, StmCell::new(fuel));
                                } else {
                                    let cell =
                                        map.get_mut(&key)?.unwrap().to_owned();
                                    track!(tx, cell);
                                    **cell += fuel;
                                }
                            }
                            Ok(())
                        })
                        .or_else(ignore_too_many_attempts)
                        .unwrap();
                        let sink_fuel = Tx::run_with_options(&tx_opts, |tx| {
                            track!(tx, map, sink);
                            for kv in map.iter() {
                                let val = kv?.1.to_owned();
                                track!(tx, val);
                                *sink.get_mut() += val.take();
                            }
                            Ok(**sink)
                        })
                        .or_else(ignore_too_many_attempts)
                        .unwrap();
                        if sink_fuel == total_fuel {
                            break;
                        }
                        if start_time.elapsed() > worker_timeout {
                            panic!("Worker timeout {worker_timeout:?}")
                        }
                    }
                })
            })
            .collect();

        for worker in workers {
            worker.join().unwrap();
        }
    });
}

fn ignore_too_many_attempts<T: Default>(e: Error) -> Result<T> {
    if let Error::TooManyTransactionRetryAttempts { .. } = e {
        Ok(T::default())
    } else {
        Err(e)
    }
}
