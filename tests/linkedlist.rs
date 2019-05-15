use std::sync::Arc;
use rlu::{Rlu, RluList, RluListNode};

#[test]
fn ll_simple() {
  let rlu: Arc<Rlu<RluListNode<usize>>> = Arc::new(Rlu::new());
  let rlu2 = rlu.clone();
  let thread = rlu.make_thread();
  let mut ll = RluList::new(rlu2);

  {
    {
      let mut lock = thread.lock();
      ll.insert(&mut lock, 2, 0);
    }
    println!("Ins 0");

    {
      let mut lock = thread.lock();
      ll.insert(&mut lock, 0, 0);
      ll.insert(&mut lock, 1, 1);
    }
    println!("Ins 1");

    {
      let mut lock = thread.lock();
      for i in 0..=2 {
        assert_eq!(ll.get(&mut lock, i).expect("Get failed"), i);
      }
    }

    {
      let mut lock = thread.lock();
      ll.delete(&mut lock, 1);
    }
    println!("Del 1");

    {
      let mut lock = thread.lock();
      assert_eq!(ll.get(&mut lock, 1).expect("Get failed"), 2);
    }

    {
      let mut lock = thread.lock();
      ll.delete(&mut lock, 0);
      assert_eq!(ll.get(&mut lock, 0).expect("Get failed"), 2);

      ll.delete(&mut lock, 0);
    }
    println!("Del 2");
  }
}
