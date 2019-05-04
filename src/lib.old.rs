#![allow(unused_imports)]

use std::sync::atomic::{compiler_fence, Ordering, AtomicUsize};
use std::pin::Pin;
use std::mem::transmute;
use std::usize;
use std::default::Default;
use std::ptr;

const RLU_CACHE_LINE_SIZE: usize = 64;
const RLU_MAX_THREADS: usize = 32;
const RLU_MAX_WRITE_SETS: usize = 20;

#[derive(Debug, Copy, Clone)]
enum ObjHeader<T> {
  Actual {
    copy: Option<*mut RluObj<T>>
  },
  Copy {
    thread_id: usize
  }
}

impl<T> Default for ObjHeader<T> {
  fn default() -> Self { ObjHeader::Actual { copy: None } }
}

#[derive(Debug, Copy, Clone)]
struct RluObj<T> {
  header: ObjHeader<T>,
  data: *mut T
}

impl<T> Default for RluObj<T> {
  fn default() -> Self { RluObj {
    header: Default::default(),
    data: ptr::null_mut() as *mut T
  } }
}

#[derive(Debug, Copy, Clone, Default)]
struct WriteLog<T> {
  entries: [RluObj<T>; RLU_MAX_WRITE_SETS],
  cur_entries: usize
}

#[derive(Debug, Copy, Clone, Default)]
pub struct RluThread<T> {
  active_log: WriteLog<T>,
  prev_log: WriteLog<T>,
  is_writer: bool,
  run_counter: usize,
  write_clock: usize,
  local_clock: usize,
  thread_id: usize,
  cur_objects: usize
}

struct RluGlobal<T> {
  thread_data: [RluThread<T>; RLU_MAX_THREADS],
  cur_threads: AtomicUsize,
  global_clock: AtomicUsize
}

impl<T: Default + Copy> RluGlobal<T> {
  fn new() -> RluGlobal<T> {
    RluGlobal {
      cur_threads: AtomicUsize::new(0),
      thread_data: [RluThread::default(); RLU_MAX_THREADS],
      global_clock: AtomicUsize::new(0)
    }
  }
}

impl<T: Default + Copy> RluThread<T> {
  fn new(global: *mut RluGlobal<T>) -> *mut RluThread<T> {
    unsafe {
      let thread_id = (*global).cur_threads.fetch_add(1, Ordering::SeqCst);
      (*global).thread_data[thread_id] = RluThread {
        active_log: WriteLog::default(),
        prev_log: WriteLog::default(),
        is_writer: false,
        run_counter: 0,
        write_clock: usize::MAX,
        local_clock: 0,
        cur_objects: 0,
        thread_id
      };

      &mut ((*global).thread_data[thread_id]) as *mut RluThread<T>
    }
  }

  fn reader_lock(&mut self, global: *mut RluGlobal<T>) {
    self.run_counter += 1;
    self.local_clock = unsafe {
      (*global).global_clock.load(Ordering::SeqCst)
    };
    self.is_writer = false;
  }

  fn reader_unlock(&mut self) {
    self.run_counter += 1;
    // TODO: commit writes?
  }

  fn dereference(&mut self, global: *mut RluGlobal<T>, obj: RluObj<T>) -> *mut T {
    match obj.header {

      // Return the provided data if it's already a copy
      ObjHeader::Copy {..} => { obj.data }

      ObjHeader::Actual {copy: copy_opt} => {
        match copy_opt {
          // Return the provided data if there is no copy
          None => { obj.data },

          Some(copy) => {
            let copy = unsafe { *copy };
            if let ObjHeader::Copy { thread_id } = copy.header {
              // Return the copy data if we have the lock on this object
              if thread_id == self.thread_id {
                copy.data
              } else {
                let other_thread = unsafe {
                  &(*global).thread_data[thread_id]
                };

                if other_thread.write_clock <= self.local_clock {
                  // If our clock is ahead of other thread's write clock,
                  // then we can steal their log object?
                  copy.data
                } else {
                  // Otherwise we can return the original object?
                  // TODO: understand this logic
                  obj.data
                }
              }
            } else {
              // Actual object pointer to RluObj mut be Copy
              unreachable!()
            }
          }
        }
      }
    }
  }

  fn alloc(&self, t: T) -> RluObj<T> {
    RluObj {
      header: ObjHeader::Actual { copy: None },
      data: Box::into_raw(Box::new(t)) as *mut T
    }
  }

  fn abort(&mut self) {
    self.run_counter += 1;
    if self.is_writer {
      // rlu unlock write log?
    }
    // retry ?
  }

  fn try_lock(&mut self, obj: &mut RluObj<T>) -> *mut T {
    self.is_writer = true;
    match obj.header {
      ObjHeader::Actual { copy: copy_opt } => {
        match copy_opt {
          Some(copy) => {
            let copy = unsafe { *copy };
            if let ObjHeader::Copy { thread_id } = copy.header {
              if self.thread_id == thread_id {
                return copy.data;
              } else {
                self.abort();
                panic!()
              }
            }
          },
          None => {
            panic!()
          }
        }
      },
      ObjHeader::Copy { .. } => panic!()
    };

    let copy = &mut self.active_log.entries[self.active_log.cur_entries];
    copy.header = ObjHeader::Copy {
      thread_id: self.thread_id
    };

    // Memcpy
    unsafe { *copy.data = *obj.data; }

    obj.header = ObjHeader::Actual { copy: Some(copy as *mut RluObj<T>) };

    panic!()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn basic() {
    let mut global = RluGlobal::new();
    let global_ref = &mut global as *mut RluGlobal<u64>;
    let rlu = RluThread::new(global_ref);
    let obj = unsafe { (*rlu).alloc(10) };

  }
}
