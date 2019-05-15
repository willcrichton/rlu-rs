#![allow(unused_mut, unused_variables, unused_assignments, dead_code)]

use crate::rlu::{Rlu, RluBounds, RluGuard, RluObject};
use std::ptr;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default)]
pub struct RluListNode<T> {
  data: T,
  next: Option<RluObject<RluListNode<T>>>,
}

pub struct RluList<T> {
  head: Option<RluObject<RluListNode<T>>>,
  rlu: Arc<Rlu<RluListNode<T>>>,
}

impl<T: RluBounds> RluList<T> {
  pub fn new(rlu: Arc<Rlu<RluListNode<T>>>) -> RluList<T> {
    RluList { head: None, rlu }
  }

  fn nth<'a>(
    &self,
    lock: &mut RluGuard<'a, RluListNode<T>>,
    index: usize,
  ) -> (
    Option<RluObject<RluListNode<T>>>,
    Option<RluObject<RluListNode<T>>>,
  ) {
    let mut prev = &None;
    let mut next = &self.head;
    let mut i = 0;

    while i < index {
      match next {
        None => panic!("Invalid index {}", index),
        Some(next_ref) => {
          let node = lock.dereference(*next_ref);
          prev = next;
          next = unsafe { &(*node).next };
          i += 1;
        }
      };
    }

    (*prev, *next)
  }

  fn nth_lock<'a>(
    &self,
    lock: &mut RluGuard<'a, RluListNode<T>>,
    index: usize,
  ) -> (
    Option<(RluObject<RluListNode<T>>, *mut RluListNode<T>)>,
    Option<(RluObject<RluListNode<T>>, *mut RluListNode<T>)>,
  ) {
    loop {
      let (prev, next) = self.nth(lock, index);

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

      return (
        prev_node.map(|p| (prev.unwrap(), p)),
        next_node.map(|n| (next.unwrap(), n)),
      );
    }
  }

  pub fn get<'a>(
    &self,
    lock: &mut RluGuard<'a, RluListNode<T>>,
    index: usize,
  ) -> Option<T> {
    let mut i = 0;
    let (_, head) = self.nth(lock, index);
    head.map(|head_ref| unsafe { *lock.dereference(head_ref) }.data)
  }

  pub fn insert<'a>(
    &mut self,
    lock: &mut RluGuard<'a, RluListNode<T>>,
    data: T,
    index: usize,
  ) {
    let (prev_opt, next_opt) = self.nth_lock(lock, index);

    let new = self.rlu.alloc(RluListNode { data, next: None });

    let new = if let Some((next, next_node)) = next_opt {
      let new_ptr = lock.try_lock(new).expect("Try lock failed");
      lock.assign_ptr(
        unsafe { (*new_ptr).next.get_or_insert(RluObject::default()) },
        next,
      );

      new
    } else {
      self.rlu.alloc(RluListNode { data, next: None })
    };

    if let Some((prev, prev_node)) = prev_opt {
      lock.assign_ptr(
        unsafe { (*prev_node).next.get_or_insert(RluObject::default()) },
        new,
      );
    } else {
      self.head = Some(new);
    }
  }

  pub fn delete<'a>(
    &mut self,
    lock: &mut RluGuard<'a, RluListNode<T>>,
    index: usize,
  ) {
    let (prev_opt, next_opt) = self.nth_lock(lock, index);

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
      self.head =
        next_opt.and_then(|(_, next_node)| unsafe { (*next_node).next });
    }
  }
}
