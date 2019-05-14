#![allow(unused_mut, unused_variables, unused_assignments, dead_code)]

use rlu::{Rlu, RluBounds, RluGuard, RluObject};
use std::sync::Arc;
use std::ptr;

#[derive(Debug, Clone, Copy)]
struct RluListNode<T> {
  data: T,
  next: *mut RluObject<RluListNode<T>>
}

impl<T: Default> Default for RluListNode<T> {
  fn default() -> Self {
    RluListNode {
      data: T::default(),
      next: ptr::null_mut()
    }
  }
}

struct RluList<T> {
  head: *mut RluObject<RluListNode<T>>,
  rlu: Arc<Rlu<RluListNode<T>>>,
}

impl<T: RluBounds> RluList<T> {
  pub fn new(rlu: Arc<Rlu<RluListNode<T>>>) -> RluList<T> {
    RluList {
      head: ptr::null_mut(),
      rlu,
    }
  }

  pub fn get<'a>(
    &self,
    lock: &mut RluGuard<'a, RluListNode<T>>,
    index: usize,
  ) -> Option<&T> {
    let mut i = 0;
    let mut head = &self.head;
    let mut data = None;
    while i < index {
      if head.is_null() {
        break;
      }

      let node = lock.dereference(unsafe { &**head });
      head = &node.next;
      data = Some(&node.data);
      i += 1;
    }

    data
  }

  pub fn insert<'a>(
    &mut self,
    lock: &mut RluGuard<'a, RluListNode<T>>,
    data: T,
    index: usize,
  ) {
    'restart: loop {
      let mut prev = ptr::null_mut();
      let mut next = self.head;
      let mut i = 0;
      loop {
        if i == index { break; }

        if next.is_null() {
          panic!("Invalid index {}", index);
        }

        let node = lock.dereference(unsafe { &*next });
        prev = next;
        next = node.next;
        i += 1;
      }

      match (
        lock.try_lock(unsafe { &mut *prev }),
        lock.try_lock(unsafe { &mut *next })
      ) {
        (None, _) | (_, None) => {
          lock.abort();
          continue 'restart;
        },
        (Some(prev_node), Some(next_node)) => {
          let mut node = self.rlu.alloc(RluListNode {
            data, next: ptr::null_mut()
          });

          // unsafe {
          //   lock.assign_ptr(
          //     &mut (*lock.try_lock(&mut node).unwrap()).next,
          //     &mut *next);
          // }
        }
      }
    }

    // match prev {
    //   Some(prev) => {
    //     let prev = prev as *const _ as *mut RluListNode<T>;
    //     self.rlu.assign_ptr(prev, node)
    //       //unsafe { (*prev).next = Some(&mut node); }
    //   }
    //   None => {
    //     self.head = Some(&mut node);
    //   }
    // }
  }
}

#[test]
fn ll_simple() {
  let rlu: Arc<Rlu<RluListNode<u64>>> = Arc::new(Rlu::new());
  let rlu2 = rlu.clone();
  let thread = rlu.make_thread();
  let ll = RluList::new(rlu2);

  {
    let mut lock = thread.lock();
  }
}
