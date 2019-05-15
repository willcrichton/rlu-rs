#![allow(unused_mut, unused_variables, unused_assignments, dead_code)]

use crate::rlu::{Rlu, RluBounds, RluObject, RluSession};
use std::sync::Arc;

#[derive(Debug, Default, Clone, Copy)]
pub struct RluListNode<T> {
  value: T,
  next: Option<RluObject<RluListNode<T>>>,
}

#[derive(Clone)]
pub struct RluList<T> {
  head: RluObject<RluListNode<T>>,
  rlu: Arc<Rlu<RluListNode<T>>>,
}

impl<T: RluBounds + PartialEq + PartialOrd> RluList<T> {
  pub fn new(rlu: Arc<Rlu<RluListNode<T>>>) -> RluList<T> {
    RluList {
      head: rlu.alloc(RluListNode::default()),
      rlu,
    }
  }

  fn find<'a>(
    &self,
    lock: &mut RluSession<'a, RluListNode<T>>,
    value: T,
  ) -> Option<(
    Option<RluObject<RluListNode<T>>>,
    Option<RluObject<RluListNode<T>>>,
  )> {
    let mut prev = &None;
    let mut next = &unsafe { (*lock.dereference(self.head)).next };

    loop {
      match next {
        None => {
          return None;
        }
        Some(next_ref) => {
          let node = lock.dereference(*next_ref);
          //println!("{:?}", unsafe { (*node) });
          if unsafe { (*node).value } >= value {
            break;
          }

          prev = next;
          next = unsafe { &(*node).next };
        }
      };
    }

    Some((*prev, *next))
  }

  fn find_lock<'a>(
    &self,
    lock: &mut RluSession<'a, RluListNode<T>>,
    value: T
  ) -> Option<(
    Option<(RluObject<RluListNode<T>>, *mut RluListNode<T>)>,
    Option<(RluObject<RluListNode<T>>, *mut RluListNode<T>)>,
  )> {
    loop {
      let (prev, next) = self.find(lock, value)?;

      let prev_node = if let Some(prev) = prev {
        match lock.try_lock(prev) {
          Some(prev_node) => Some(prev_node),
          None => {
            lock.abort();
            continue;
          }
        }
      } else {
        None
      };

      let next_node = if let Some(next) = next {
        match lock.try_lock(next) {
          Some(next_node) => Some(next_node),
          None => {
            lock.abort();
            continue;
          }
        }
      } else {
        None
      };

      return Some((
        prev_node.map(|p| (prev.unwrap(), p)),
        next_node.map(|n| (next.unwrap(), n)),
      ));
    }
  }

  pub fn contains<'a>(
    &self,
    lock: &mut RluSession<'a, RluListNode<T>>,
    value: T,
  ) -> Option<()> {
    let (_, head) = self.find(lock, value)?;
    head.and_then(|head_ref| {
      if unsafe { *lock.dereference(head_ref) }.value == value {
        Some(())
      } else {
        None
      }
    })
  }

  pub fn insert<'a>(
    &mut self,
    lock: &mut RluSession<'a, RluListNode<T>>,
    value: T,
  ) -> Option<()> {
    let mut head_node = None;
    let mut prev_opt;
    let mut next_opt;
    loop {
      let (prev_opt2, next_opt2) = match self.find_lock(lock, value) {
        None => (None, None),
        Some(opts) => opts
      };

      if let None = prev_opt2 {
        match lock.try_lock(self.head) {
          None => {
            lock.abort();
            continue;
          }
          Some(head_node2) => {
            head_node = Some(head_node2);
          }
        }
      }

      prev_opt = prev_opt2;
      next_opt = next_opt2;
      break;
    }

    let new = self.rlu.alloc(RluListNode { value, next: None });

    let new = if let Some((next, next_node)) = next_opt {
      let new_ptr = lock.try_lock(new).expect("Try lock failed");
      lock.assign_ptr(
        unsafe { (*new_ptr).next.get_or_insert(RluObject::default()) },
        next,
      );

      new
    } else {
      self.rlu.alloc(RluListNode { value, next: None })
    };

    if let Some((prev, prev_node)) = prev_opt {
      lock.assign_ptr(
        unsafe { (*prev_node).next.get_or_insert(RluObject::default()) },
        new,
      );
    } else {
      unsafe {
        (*head_node.unwrap()).next = Some(new);
      }
    }

    Some(())
  }

  pub fn delete<'a>(
    &mut self,
    lock: &mut RluSession<'a, RluListNode<T>>,
    value: T,
  ) -> Option<()> {
    let mut head_node = None;
    let mut prev_opt;
    let mut next_opt;
    loop {
      let (prev_opt2, next_opt2) = self.find_lock(lock, value)?;
      if let None = prev_opt2 {
        match lock.try_lock(self.head) {
          None => {
            continue;
          }
          Some(head_node2) => {
            head_node = Some(head_node2);
          }
        }
      }
      prev_opt = prev_opt2;
      next_opt = next_opt2;
      break;
    }

    match next_opt {
      Some((_, next_node)) => {
        if unsafe { (*next_node).value } != value {
          return None;
        }
      },
      None => { return None; }
    };

    if let Some((prev, prev_node)) = prev_opt {
      if let Some((_, next_node)) = next_opt {
        if let Some(next2) = unsafe { (*next_node).next } {
          lock.assign_ptr(
            unsafe { (*prev_node).next.get_or_insert(RluObject::default()) },
            next2,
          );
        } else {
          unsafe {
            (*prev_node).next = None;
          }
        }
      } else {
        unsafe {
          (*prev_node).next = None;
        }
      }
    } else {
      unsafe {
        (*head_node.unwrap()).next =
          next_opt.and_then(|(_, next_node)| (*next_node).next);
      }
    }

    Some(())
  }

  pub fn to_string<'a>(&self, lock: &mut RluSession<'a, RluListNode<T>>) -> String {
    let mut cur = &unsafe { (*lock.dereference(self.head)).next };
    let mut s = String::new();

    loop {
      match cur {
        None => {
          break;
        }
        Some(cur_ref) => {
          let node = lock.dereference(*cur_ref);
          s += &format!(" --> {:?}", unsafe { *node });
          cur = unsafe { &(*node).next };
        }
      };
    }

    return s;
  }

}
