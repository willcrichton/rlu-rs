extern crate rand;

use rlu::{Rlu, RluList, RluListNode};
use std::sync::Arc;
use std::thread;

use rand::{thread_rng, Rng};

#[test]
fn ll_simple() {
  let rlu: Arc<Rlu<RluListNode<usize>>> = Arc::new(Rlu::new());
  let rlu2 = rlu.clone();
  let thread = rlu.make_thread();
  let mut ll = RluList::new(rlu2);

  {
    {
      let mut lock = thread.lock();
      assert!(ll.contains(&mut lock, 0).is_none());
      assert!(ll.delete(&mut lock, 0).is_none());
      assert!(ll.insert(&mut lock, 2).is_some());
      println!("Ins 0: {}", ll.to_string(&mut lock));
    }

    {
      let mut lock = thread.lock();
      assert!(ll.insert(&mut lock, 0).is_some());
      assert!(ll.insert(&mut lock, 1).is_some());
      println!("Ins 1: {}", ll.to_string(&mut lock));
    }

    {
      let mut lock = thread.lock();
      for i in 0..=2 {
        assert!(ll.contains(&mut lock, i).is_some());
      }

      assert!(ll.contains(&mut lock, 5).is_none());
      println!("Contains");
    }

    {
      let mut lock = thread.lock();
      assert!(ll.delete(&mut lock, 1).is_some());
      println!("Del 1: {}", ll.to_string(&mut lock));
    }

    {
      let mut lock = thread.lock();
      assert!(ll.contains(&mut lock, 1).is_none());
    }

    {
      let mut lock = thread.lock();
      assert!(ll.delete(&mut lock, 0).is_some());
      assert!(ll.contains(&mut lock, 0).is_none());

      assert!(ll.delete(&mut lock, 2).is_some());
      println!("Del 2: {}", ll.to_string(&mut lock));
    }
  }
}

#[test]
fn ll_thread() {
  let rlu: Arc<Rlu<RluListNode<usize>>> = Arc::new(Rlu::new());
  let ll = RluList::new(rlu);

  // TODO: concurrency test

  let reader = || {
    let mut ll = ll.clone();
    thread::spawn(move || {
      let mut rng = thread_rng();
      for _ in 0..100 {
        let i = rng.gen_range(0, 50) * 2;
        assert!(ll.contains(i).is_some());
      }
    })
  };

  let writer = || {
    let mut ll = ll.clone();
    thread::spawn(move || {});
  };
}
