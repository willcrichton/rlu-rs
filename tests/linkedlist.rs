extern crate rand;

use rlu::{Rlu, RluList, RluListNode};
use std::sync::Arc;
use std::thread;

use rand::{random, thread_rng, Rng};

#[test]
fn ll_simple() {
  let rlu: Arc<Rlu<RluListNode<usize>>> = Arc::new(Rlu::new());
  let thread = rlu.make_thread();
  let mut ll = RluList::new(rlu.clone());

  {
    {
      assert!(ll.contains(0).is_none());
      assert!(ll.delete(0).is_none());
      assert!(ll.insert(2).is_some());
      println!("Ins 0: {}", ll.to_string());
    }

    {
      assert!(ll.insert(0).is_some());
      assert!(ll.insert(1).is_some());
      println!("Ins 1: {}", ll.to_string());
    }

    {
      for i in 0..=2 {
        assert!(ll.contains(i).is_some());
      }

      assert!(ll.contains(5).is_none());
      println!("Contains");
    }

    {
      assert!(ll.delete(1).is_some());
      println!("Del 1: {}", ll.to_string());
    }

    {
      assert!(ll.contains(1).is_none());
    }

    {
      assert!(ll.delete(0).is_some());
      assert!(ll.contains(0).is_none());

      assert!(ll.delete(2).is_some());
      println!("Del 2: {}", ll.to_string());
    }
  }
}

#[test]
fn ll_thread() {
  let rlu: Arc<Rlu<RluListNode<usize>>> = Arc::new(Rlu::new());
  let mut ll = RluList::new(rlu.clone());

  {
    let thread = rlu.make_thread();
    for i in 0..1000 {
      assert!(ll.insert(i).is_some());
    }
  }

  let reader = || {
    let rlu = rlu.clone();
    let mut ll = ll.clone();
    thread::spawn(move || {
      let mut rng = thread_rng();

      for _ in 0..10000 {
        let i = rng.gen_range(0, 500) * 2;
        assert!(ll.contains(i).is_some());
      }
    })
  };

  let writer = || {
    let rlu = rlu.clone();
    let mut ll = ll.clone();
    thread::spawn(move || {
      let mut rng = thread_rng();

      for i in 0..1000 {
        let i = rng.gen_range(0, 499) * 2 + 1;
        if random() {
          ll.insert(i);
        } else {
          ll.delete(i);
        }
      }
    })
  };

  let readers: Vec<_> = (0..16).map(|_| reader()).collect();
  let writers: Vec<_> = (0..4).map(|_| writer()).collect();

  for t in readers {
    t.join().unwrap();
  }

  for t in writers {
    t.join().unwrap();
  }
}
