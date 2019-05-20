#![feature(test)]

extern crate rand;
extern crate test;

use rand::{thread_rng, Rng};
use rlu::RluList;
use std::thread;
use std::time::Instant;

#[derive(Clone, Copy)]
struct BenchOpts {
  num_threads: usize,
  initial_size: usize,
  range: usize,
  timeout: u128,
  write_frac: f64,
  insert_frac: f64,
  num_iters: usize,
}

#[derive(Clone, Copy, Default, Debug)]
struct BenchResult {
  reads: usize,
  read_times: u128,
  inserts: usize,
  insert_times: u128,
  deletes: usize,
  delete_times: u128,
  ops: usize,
  op_times: u128,
}

fn ll_readwrite(ll: RluList<usize>, opts: BenchOpts) -> BenchResult {
  let worker = || {
    let mut ll = ll.clone();
    thread::spawn(move || {
      let mut rng = thread_rng();
      let mut result = BenchResult::default();
      let start = Instant::now();
      loop {
        if start.elapsed().as_millis() >= opts.timeout {
          break;
        }

        let iter_start = Instant::now();
        let i = rng.gen_range(0, opts.range);
        if rng.gen::<f64>() > opts.write_frac {
          let start = Instant::now();
          ll.contains(i);
          result.reads += 1;
          result.read_times += start.elapsed().as_nanos();
        } else {
          if rng.gen::<f64>() > opts.insert_frac {
            let start = Instant::now();
            ll.insert(i);
            result.inserts += 1;
            result.insert_times += start.elapsed().as_nanos();
          } else {
            let start = Instant::now();
            ll.delete(i);
            result.deletes += 1;
            result.delete_times += start.elapsed().as_nanos();
          }
        }

        result.ops += 1;
        result.op_times += iter_start.elapsed().as_nanos();
      }

      result
    })
  };

  let threads: Vec<_> = (0..opts.num_threads).map(|_| worker()).collect();
  threads.into_iter().map(|t| t.join().unwrap()).fold(
    BenchResult::default(),
    |mut acc, res| {
      acc.ops += res.ops;
      acc.reads += res.reads;
      acc.inserts += res.inserts;
      acc.deletes += res.deletes;
      acc.op_times += res.op_times;
      acc.read_times += res.read_times;
      acc.insert_times += res.insert_times;
      acc.delete_times += res.delete_times;
      acc
    },
  )
}

fn benchmark() {
  println!("write_frac,num_threads,throughput");
  for write_frac in &[0.02, 0.2, 0.4] {
    for num_threads in 1..=8 {
      let opts = BenchOpts {
        num_threads: num_threads,
        write_frac: *write_frac,
        insert_frac: 0.5,
        initial_size: 256,
        range: 512,
        timeout: 10000,
        num_iters: 3,
      };

      let ops: Vec<_> = (0..opts.num_iters)
        .map(|_| {
          let mut ll = RluList::new();
          let mut rng = thread_rng();
          while ll.len() < opts.initial_size {
            let i = rng.gen_range(0, opts.range);
            ll.insert(i);
          }

          ll_readwrite(ll, opts)
        })
        .collect();

      let avg: f64 = (ops.iter().map(|res| res.ops).sum::<usize>() as f64)
        / (ops.len() as f64);
      let throughput = avg / ((opts.timeout * 1000) as f64);

      println!("{},{},{}", write_frac, num_threads, throughput);
      // println!("ops: {:.0}, throughput: {:.3}", avg, throughput);
      // println!(
      //   "avg read: {:.2}us",
      //   (ops[0].read_times as f64) / (ops[0].reads as f64) / 1000.
      // );
      // println!(
      //   "avg insert: {:.2}us",
      //   (ops[0].insert_times as f64) / (ops[0].inserts as f64) / 1000.
      // );
      // println!(
      //   "avg delete: {:.2}us",
      //   (ops[0].delete_times as f64) / (ops[0].deletes as f64) / 1000.
      // );
      // println!(
      //   "avg op: {:.2}us",
      //   (ops[0].op_times as f64) / (ops[0].ops as f64) / 1000.
      // );
    }
  }
}

fn main() {
  benchmark();
}
