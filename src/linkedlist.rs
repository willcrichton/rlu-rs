#![allow(unused_mut, unused_variables, unused_assignments, dead_code)]

use crate::rlu::{Rlu, RluBounds, RluObject, RluSession, RluThread};
use std::sync::Arc;

#[derive(Debug, Default, Clone, Copy)]
pub struct RluListNode<T> {
  value: T,
  next: Option<RluObject<RluListNode<T>>>,
}

pub struct RluList<T> {
  head: RluObject<RluListNode<T>>,
  thread: *mut RluThread<RluListNode<T>>,
  rlu: Arc<Rlu<RluListNode<T>>>,
}

unsafe impl<T> Send for RluList<T> {}
unsafe impl<T> Sync for RluList<T> {}

impl<T: RluBounds + PartialEq + PartialOrd> RluList<T> {
  pub fn new(rlu: Arc<Rlu<RluListNode<T>>>) -> RluList<T> {
    RluList {
      head: rlu.alloc(RluListNode::default()),
      thread: rlu.make_thread() as *mut RluThread<RluListNode<T>>,
      rlu,
    }
  }

  fn find<'a>(
    &self,
    lock: &mut RluSession<'a, RluListNode<T>>,
    value: T,
  ) -> (
    Option<RluObject<RluListNode<T>>>,
    Option<RluObject<RluListNode<T>>>,
  ) {
    let mut prev = &None;
    let mut next = &unsafe { (*lock.dereference(self.head)).next };

    loop {
      match next {
        None => {
          break;
        }
        Some(next_ref) => {
          let node = lock.dereference(*next_ref);
          if unsafe { (*node).value } >= value {
            break;
          }

          prev = next;
          next = unsafe { &(*node).next };
        }
      };
    }

    (*prev, *next)
  }

  fn find_lock<'a>(
    &self,
    value: T,
    return_if_found: bool
  ) -> Option<(
    Option<(RluObject<RluListNode<T>>, *mut RluListNode<T>)>,
    Option<(RluObject<RluListNode<T>>, *mut RluListNode<T>)>,
    Option<*mut RluListNode<T>>,
    RluSession<'a, RluListNode<T>>,
  )> {
    loop {
      let mut lock = unsafe { (*self.thread).lock() };
      let (prev, next) = self.find(&mut lock, value);

      if let Some(next) = next {
        let found = unsafe { (*lock.dereference(next)).value } == value;
        if (return_if_found && found) || (!return_if_found && !found)  {
          return None;
        }
      } else if !return_if_found {
        return None;
      }

      let (head_node, prev_node) = if let Some(prev) = prev {
        match lock.try_lock(prev) {
          Some(prev_node) => (None, Some(prev_node)),
          None => {
            lock.abort();
            continue;
          }
        }
      } else {
        match lock.try_lock(self.head) {
          Some(head_node) => (Some(head_node), None),
          None => {
            lock.abort();
            continue;
          }
        }
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
        head_node,
        lock,
      ));
    }
  }

  pub fn contains(&self, value: T) -> Option<()> {
    let mut lock = unsafe { (*self.thread).lock() };
    let (_, head) = self.find(&mut lock, value);
    head.and_then(|head_ref| {
      if unsafe { *lock.dereference(head_ref) }.value == value {
        Some(())
      } else {
        None
      }
    })
  }

  pub fn len(&self) -> usize {
    let mut lock = unsafe { (*self.thread).lock() };
    let mut cur = &unsafe { (*lock.dereference(self.head)).next };
    let mut i = 0;

    loop {
      match cur {
        None => {
          break;
        }
        Some(cur_ref) => {
          let node = lock.dereference(*cur_ref);
          i += 1;
          cur = unsafe { &(*node).next };
        }
      };
    }

    return i;
  }

  pub fn insert(&mut self, value: T) -> Option<()> {
    let (prev_opt, next_opt, head_node, mut lock) = self.find_lock(value, true)?;

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

  pub fn delete(&mut self, value: T) -> Option<()> {
    let (prev_opt, next_opt, head_node, mut lock) = self.find_lock(value, false)?;

    if let Some((prev, prev_node)) = prev_opt {
      if let Some((_, next_node)) = next_opt {
        if let Some(next2) = unsafe { (*next_node).next } {
          lock.assign_ptr(
            unsafe { (*prev_node).next.get_or_insert_with(|| RluObject::default()) },
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

    if let Some((next, _)) = next_opt {
      unsafe { (*self.thread).free(next); }
    }

    Some(())
  }

  pub fn to_string(&self) -> String {
    let mut lock = unsafe { (*self.thread).lock() };
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

impl<T: RluBounds> Clone for RluList<T> {
  fn clone(&self) -> Self {
    RluList {
      head: self.head,
      thread: self.rlu.make_thread(),
      rlu: self.rlu.clone(),
    }
  }
}
