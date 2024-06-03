use assert_matches::assert_matches;
use naive_stm::{track, StmQueue, Tx};
use rand::seq::SliceRandom;
use std::{
    iter, thread,
    time::{Duration, Instant},
};

fn drain_queue<T: Clone + 'static>(queue: &StmQueue<T>) -> Vec<T> {
    Tx::run(|tx| {
        track! {tx, queue};
        let mut items = vec![];
        while let Some(item) = queue.pop()? {
            items.push(item)
        }
        Ok(items)
    })
    .unwrap()
}

#[test]
fn queue_forwarding() {
    let number_of_items = 18;
    let source: StmQueue<_> = (220..220 + number_of_items).collect();
    let sink = StmQueue::new();

    let number_of_broker_queues = 31;
    let broker_queues: Vec<_> = iter::repeat_with(StmQueue::new)
        .take(number_of_broker_queues)
        .collect();
    let queues: Vec<_> = iter::once(&source)
        .chain(&broker_queues)
        .chain([&sink])
        .collect();
    let mut pipelines: Vec<_> = queues.windows(2).collect();
    pipelines.shuffle(&mut rand::thread_rng());
    let pipelines_per_worker = 5;
    let worker_timeout = Duration::from_secs(5);

    thread::scope(|scope| {
        let workers: Vec<_> = pipelines
            .chunks(pipelines_per_worker)
            .enumerate()
            .map(|(worker_num, worker_pipelines)| {
                let items_to_forward = number_of_items * worker_pipelines.len();
                scope.spawn(move || {
                    let start_time = Instant::now();
                    let mut items_forwarded = 0;
                    loop {
                        items_forwarded += Tx::run(|tx| {
                            let mut tx_items_forwarded = 0;
                            for pipeline in worker_pipelines {
                                let mut from_queue = tx.track(pipeline[0])?;
                                let mut to_queue = tx.track(pipeline[1])?;
                                if let Some(item) = from_queue.pop()? {
                                    println!("Worker {worker_num}: item `{item}`",);
                                    to_queue.push(item);
                                    tx_items_forwarded += 1;

                                    assert!(!to_queue.is_empty()?);
                                    assert_matches!(to_queue.peek()?, Some(_));
                                } else {
                                    assert!(from_queue.is_empty()?);
                                    assert_matches!(from_queue.peek()?, None);
                                }
                            }
                            Ok(tx_items_forwarded)
                        })
                        .unwrap();
                        println!( "Worker {worker_num}: {items_forwarded}/{items_to_forward}");
                        if items_forwarded == items_to_forward {
                            break;
                        }
                        if start_time.elapsed() > worker_timeout {
                            panic!("Worker timeout {worker_timeout:?}")
                        }
                        thread::sleep(Duration::from_millis(1))
                    }})
            })
            .collect();

        for worker in workers {
            worker.join().unwrap();
        }
    });

    assert_eq!(drain_queue(&source), vec![]);
    for broker_queue in &broker_queues {
        assert_eq!(drain_queue(broker_queue), vec![]);
    }
    assert_eq!(
        drain_queue(&sink),
        vec![
            220, 221, 222, 223, 224, 225, 226, 227, 228, 229, 230, 231, 232,
            233, 234, 235, 236, 237
        ]
    );
}
