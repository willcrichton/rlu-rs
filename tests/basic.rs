#![allow(unused_mut, unused_variables, unused_imports)]

use std::sync::mpsc;
use std::sync::Arc;
use std::{thread, time};

use rlu::Rlu;

#[test]
fn basic_single() {
  let rlu: Arc<Rlu<u64>> = Arc::new(Rlu::new());
  let mut obj = rlu.alloc(3);
  let thread = rlu.thread();

  {
    let mut lock = thread.session();

    // Object should have original value after first deref
    {
      let n = lock.read_lock(obj);
      unsafe {
        assert_eq!(*n, 3);
      }
    }

    // Object should still have same value, but now it's safe to write
    // We should have a copy at this point
    {
      let n = lock.write_lock(obj).unwrap();
      unsafe {
        assert_eq!(*n, 3);
        *n += 1;
      }
    }

    // Subsequent derefs in same thread should refer to the copy, observing
    // the new value
    {
      let n = lock.read_lock(obj);
      unsafe {
        assert_eq!(*n, 4);
      }
    }
  }

  {
    // Start a new session
    let mut lock = thread.session();

    // Read should observed flushed change
    {
      let n = lock.read_lock(obj);
      unsafe {
        assert_eq!(*n, 4);
      }
    }
  }
}

#[test]
fn basic_overlapping_reader_writer() {
  let rlu: Arc<Rlu<u64>> = Arc::new(Rlu::new());
  let mut obj = rlu.alloc(3);

  let thread0 = rlu.thread();
  let thread1 = rlu.thread();

  {
    let mut lock1 = thread1.session();
    let mut lock0 = thread0.session();

    {
      let n1: *const u64 = lock0.read_lock(obj);
      unsafe {
        assert_eq!(*n1, 3);
      }
    }

    // Thread 1 should be working on a copy
    {
      let n2: *mut u64 = lock1.write_lock(obj).unwrap();
      unsafe {
        assert_eq!(*n2, 3);
        *n2 += 1;
      }
    }

    // Thread 0 should be working on the original
    {
      let n1: *const u64 = lock0.read_lock(obj);
      unsafe {
        assert_eq!(*n1, 3);
      }
    }

    // Thread 0 exits, allowing thread 1 to flush writes
  }

  let mut lock = thread0.session();
  unsafe {
    assert_eq!(*lock.read_lock(obj), 4);
  }
}

#[test]
fn basic_thread() {
  let rlu: Arc<Rlu<u64>> = Arc::new(Rlu::new());
  let mut obj = rlu.alloc(0);

  let reader = |id: u64| {
    let rlu = rlu.clone();
    thread::spawn(move || {
      let thr = rlu.thread();

      for _ in 0..100 {
        let mut lock = thr.session();
        let n = lock.read_lock(obj);
        let x = unsafe { *n };
        thread::sleep(time::Duration::from_millis(10));
        assert_eq!(unsafe { *n }, x);
      }

      println!("Reader {} exit", id);
    })
  };

  let writer = |id: u64| {
    let rlu = rlu.clone();
    thread::spawn(move || {
      let thr = rlu.thread();

      for i in 0..1000 {
        // if i % 100 == 0 {
        //   println!("{}: {}", id, i);
        // }
        loop {
          let mut lock = thr.session();
          if let Some(n) = lock.write_lock(obj) {
            unsafe {
              *n += 1;
            }
            break;
          } else {
            lock.abort();
          }
        }
      }

      println!("Writer {} exit", id);
    })
  };

  let num_readers = 16;
  let num_writers = 2;

  let readers: Vec<_> = (0..num_readers).map(|i| reader(i)).collect();
  let writers: Vec<_> = (0..num_writers).map(|i| writer(i)).collect();

  for t in readers {
    t.join().expect("Reader panicked");
  }

  for t in writers {
    t.join().expect("Writer panicked");
  }

  let thr = rlu.thread();
  let mut lock = thr.session();
  assert_eq!(unsafe { *lock.read_lock(obj) }, 1000 * num_writers);
}
