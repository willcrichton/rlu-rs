#![feature(test)]

extern crate rand;
extern crate test;

use rand::{thread_rng, Rng};
use rlu::{Rlu, RluList, RluListNode};
use std::sync::Arc;
use std::thread;
use std::time::{Instant, Duration};
use test::black_box;

#[derive(Clone, Copy)]
struct BenchOpts {
  num_threads: usize,
  initial_size: usize,
  range: usize,
  timeout: u128,
  write_frac: f64,
  insert_frac: f64,
}

fn ll_readwrite(
  ll: RluList<usize>,
  rlu: Arc<Rlu<RluListNode<usize>>>,
  opts: BenchOpts,
) -> usize {
  let worker = || {
    let rlu = rlu.clone();
    let mut ll = ll.clone();

    thread::spawn(move || {
      let mut rng = thread_rng();
      let mut ops = 0;
      let start = Instant::now();
      loop {
        if start.elapsed().as_millis() >= opts.timeout {
          break;
        }

        let i = rng.gen_range(0, opts.range);
        if rng.gen::<f64>() > opts.write_frac {
          black_box(ll.contains(i));
        } else {
          if rng.gen::<f64>() > opts.insert_frac {
            black_box(ll.insert(i));
          } else {
            black_box(ll.delete(i));
          }
        }

        ops += 1;
      }

      ops
    })
  };

  let threads: Vec<_> = (0..opts.num_threads).map(|_| worker()).collect();
  threads.into_iter().map(|t| t.join().unwrap()).sum()
}

fn benchmark() {
  for write_frac in &[0.02, 0.2, 0.4] {
    for num_threads in 1..=8 {
      let opts = BenchOpts {
        num_threads: num_threads,
        write_frac: *write_frac,
        insert_frac: 0.5,
        initial_size: 256,
        range: 512,
        timeout: 2000,
      };

      let ops: Vec<_> = (0..3).map(|_| {
        let rlu: Arc<Rlu<RluListNode<usize>>> = Arc::new(Rlu::new());
        let mut ll = RluList::new(rlu.clone());

        {
          let thread = rlu.make_thread();
          let mut rng = thread_rng();
          while ll.len() < opts.initial_size {
            let i = rng.gen_range(0, opts.range);
            black_box(ll.insert(i));
          }
        }

        ll_readwrite(ll, rlu, opts)
      }).collect();

      let avg: f64 = (ops.iter().sum::<usize>() as f64) / (ops.len() as f64);
      let throughput = avg / ((opts.timeout * 1000) as f64) ;

      println!("ops: {:.0}, throughput: {:.3}", avg, throughput);
      println!("{},{},{}", write_frac, num_threads, throughput);
    }
  }
}

fn main() {
  benchmark();
}
