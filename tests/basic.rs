#![allow(unused_mut, unused_variables, unused_imports)]

use std::thread;
use std::sync::mpsc;
use std::sync::Arc;

use rlu::Rlu;

#[test]
fn basic() {
  let rlu: Arc<Rlu<u64>> = Arc::new(Rlu::new());
  let mut obj = rlu.alloc(3);
  let thread = rlu.make_thread();

  {
    let mut lock = thread.lock();

    // Object should have original value after first deref
    {
      let n = lock.dereference(&obj);
      assert_eq!(*n, 3);
    }

    // Object should still have same value, but now it's safe to write
    // We should have a copy at this point
    {
      let n = lock.try_lock(&mut obj).unwrap();
      unsafe {
        assert_eq!(*n, 3);
        *n += 1;
      }
    }

    // Subsequent derefs in same thread should refer to the copy, observing
    // the new value
    {
      let n = lock.dereference(&obj);
      assert_eq!(*n, 4);
    }
  }

  {
    // Start a new session
    let mut lock = thread.lock();

    // Read should observed flushed change
    {
      let n = lock.dereference(&obj);
      assert_eq!(*n, 4);
    }
  }
}

#[test]
fn concurrent_reader_writer() {
  let rlu: Arc<Rlu<u64>> = Arc::new(Rlu::new());
  let mut obj = rlu.alloc(3);

  let thread0 = rlu.make_thread();
  let thread1 = rlu.make_thread();

  {
    let mut lock1 = thread1.lock();
    let mut lock0 = thread0.lock();

    {
      let n1: &u64 = lock0.dereference(&obj);
      assert_eq!(*n1, 3);
    }

    // Thread 1 should be working on a copy
    {
      let n2: *mut u64 = lock1.try_lock(&mut obj).unwrap();
      unsafe {
        assert_eq!(*n2, 3);
        *n2 += 1;
      }
    }

    // Thread 0 should be working on the original
    {
      let n1: &u64 = lock0.dereference(&obj);
      assert_eq!(*n1, 3);
    }

    // Thread 0 exits, allowing thread 1 to flush writes
  }

  let mut lock = thread0.lock();
  assert_eq!(*lock.dereference(&obj), 4);
}

// #[test]
// fn thread() {
//   let rlu: Arc<Rlu<u64>> = Arc::new(Rlu::new());
//   let mut obj1 = RluObject::new(3);
//   let mut obj2 = RluObject::new(5);

//   let rlu1 = rlu.clone();
//   let t0 = thread::spawn(move || {
//     let thread0 = rlu1.make_thread();
//     thread0.reader_lock(&rlu1);

//     let n2: &mut u64 = thread0.try_lock(&rlu, &mut obj1).unwrap();
//     assert_eq!(*n2, 3);
//     *n2 += 1;

//     thread0.reader_unlock(&rlu);
//   });
// }
