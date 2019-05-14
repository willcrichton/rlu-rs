#![allow(unused_imports, dead_code, unused_variables)]

use std::cell::RefCell;
use std::fmt::Debug;
use std::mem::transmute;
use std::ptr;
use std::sync::atomic::{compiler_fence, AtomicUsize, Ordering};
use std::sync::Arc;
use std::usize;
use std::cell::UnsafeCell;

const RLU_MAX_LOG_SIZE: usize = 32;
const RLU_MAX_THREADS: usize = 32;

#[derive(Default, Clone, Copy)]
pub struct ObjOriginal<T> {
  copy: Option<*mut ObjCopy<T>>,
  data: T,
}

#[derive(Clone, Copy)]
pub struct ObjCopy<T> {
  thread_id: usize,
  original: *mut ObjOriginal<T>,
  data: T,
}

unsafe impl<T> Send for ObjOriginal<T> {}
unsafe impl<T> Sync for ObjOriginal<T> {}

unsafe impl<T> Send for ObjCopy<T> {}
unsafe impl<T> Sync for ObjCopy<T> {}

#[derive(Clone, Copy)]
pub enum RluObject<T> {
  Original(ObjOriginal<T>),
  Copy(ObjCopy<T>),
}

pub struct RluCell<T> {
  cell: UnsafeCell<RluObject<T>>
}

impl<T: Default> Default for ObjCopy<T> {
  fn default() -> Self {
    ObjCopy {
      thread_id: 0,
      data: T::default(),
      original: ptr::null_mut(),
    }
  }
}

#[derive(Default, Clone, Copy)]
struct WriteLog<T> {
  entries: [ObjCopy<T>; RLU_MAX_LOG_SIZE],
  num_entries: usize,
}

#[derive(Clone, Copy)]
pub struct RluThread<T> {
  active_log: WriteLog<T>,
  prev_log: WriteLog<T>,
  is_writer: bool,
  write_clock: usize,
  local_clock: usize,
  run_counter: usize,
  thread_id: usize,
  global: *const Rlu<T>,
}

pub struct Rlu<T> {
  global_clock: AtomicUsize,
  threads: [RluThread<T>; RLU_MAX_THREADS],
  num_threads: AtomicUsize,
}

pub struct RluGuard<'a, T: RluBounds>(&'a mut RluThread<T>);

pub trait RluBounds: Default + Copy + Debug {}
impl<T: Default + Copy + Debug> RluBounds for T {}

impl<T: RluBounds> WriteLog<T> {
  fn next_entry(&mut self) -> &mut ObjCopy<T> {
    let i = self.num_entries;
    self.num_entries += 1;
    &mut self.entries[i]
  }
}

impl<T: RluBounds> Rlu<T> {
  pub fn new() -> Rlu<T> {
    Rlu {
      global_clock: AtomicUsize::new(0),
      threads: [RluThread::new(); RLU_MAX_THREADS],
      num_threads: AtomicUsize::new(0),
    }
  }

  pub fn make_thread(&self) -> &mut RluThread<T> {
    let thread_id = self.num_threads.fetch_add(1, Ordering::SeqCst);
    let thread: *mut RluThread<T> =
      &self.threads[thread_id] as *const RluThread<T> as *mut RluThread<T>;
    let thread: &mut RluThread<T> = unsafe { &mut *thread };
    thread.thread_id = thread_id;
    thread.global = self as *const Rlu<T>;
    return thread;
  }

  fn get_thread(&self, index: usize) -> *mut RluThread<T> {
    &self.threads[index] as *const RluThread<T> as *mut RluThread<T>
  }

  pub fn alloc(&self, data: T) -> RluObject<T> {
    // TODO: save object pointer to deallocate on Drop
    RluObject::Original(ObjOriginal { copy: None, data })
  }
}

macro_rules! log {
  ($self:expr, $e:expr) => {
    let s: String = $e.into();
    if false {
      println!("Thread {}: {}", $self.thread_id, s);
    }
  };
}

impl<'a, T: RluBounds> RluGuard<'a, T> {
  pub fn dereference<'b>(&mut self, obj: &'b RluObject<T>) -> &'b T {
    log!(self.0, "dereference");
    let global = unsafe { &*self.0.global };
    match obj {
      RluObject::Copy(ref copy) => &copy.data,
      RluObject::Original(ref orig) => match orig.copy {
        None => &orig.data,
        Some(copy) => {
          let copy = unsafe { &*copy };
          if self.0.thread_id == copy.thread_id {
            &copy.data
          } else {
            let thread = unsafe { &*global.get_thread(copy.thread_id) };
            if thread.write_clock <= self.0.local_clock {
              &copy.data
            } else {
              &orig.data
            }
          }
        }
      },
    }
  }

  pub fn try_lock(&mut self, obj: &mut RluObject<T>) -> Option<*mut T> {
    log!(self.0, "try_lock");
    let global = unsafe { &*self.0.global };
    self.0.is_writer = true;
    let orig = match obj {
      RluObject::Original(ref mut orig) => match orig.copy {
        Some(copy) => {
          let copy = unsafe { &mut *copy };
          if self.0.thread_id == copy.thread_id {
            return Some(&mut copy.data);
          } else {
            self.0.abort();
            return None;
          }
        }
        None => orig,
      },

      RluObject::Copy(copy) => unsafe { &mut *copy.original },
    };

    let copy = self.0.active_log.next_entry();
    copy.thread_id = self.0.thread_id;
    copy.original = orig as *mut ObjOriginal<T>;
    copy.data = orig.data;

    orig.copy = Some(copy as *mut ObjCopy<T>);

    Some(&mut copy.data as *mut T)
  }

  pub fn abort(&mut self) {
    self.0.abort()
  }

  pub fn assign_ptr(
    &self,
    ptr: &mut *mut RluObject<T>,
    obj: &mut RluObject<T>,
  ) {
    // *ptr = &mut (match obj {
    //   RluObject::Original(orig) => *obj,
    //   RluObject::Copy(copy) => unsafe { *copy.original }
    // }) as *mut T;
  }
}

impl<'a, T: RluBounds> Drop for RluGuard<'a, T> {
  fn drop(&mut self) {
    self.0.unlock();
  }
}

impl<T: RluBounds> RluThread<T> {
  fn new() -> RluThread<T> {
    RluThread {
      active_log: WriteLog::default(),
      prev_log: WriteLog::default(),
      is_writer: false,
      write_clock: usize::MAX,
      local_clock: 0,
      run_counter: 0,
      thread_id: 0,
      global: ptr::null(),
    }
  }

  pub fn lock<'a>(&'a mut self) -> RluGuard<'a, T> {
    let global = unsafe { &*self.global };
    self.run_counter += 1;
    self.local_clock = global.global_clock.load(Ordering::SeqCst);
    self.is_writer = false;
    RluGuard(self)
  }

  fn unlock(&mut self) {
    self.run_counter += 1;

    if self.is_writer {
      let global = unsafe { &*self.global };
      self.write_clock = global.global_clock.fetch_add(1, Ordering::SeqCst) + 1;
      self.synchronize();
      self.writeback_logs();
      self.unlock_write_log();
      self.write_clock = usize::MAX;
      self.swap_logs();
    }
  }

  fn writeback_logs(&mut self) {
    log!(self, "writeback_logs");
    for i in 0..self.active_log.num_entries {
      let copy = &mut self.active_log.entries[i];
      log!(self, format!("copy {:?}", copy.data));
      unsafe {
        (*copy.original).data = copy.data;
      }
    }
  }

  fn unlock_write_log(&mut self) {
    log!(self, "unlock_write_log");
    for i in 0..self.active_log.num_entries {
      unsafe {
        (*self.active_log.entries[i].original).copy = None;
      }
    }
  }

  fn swap_logs(&mut self) {
    log!(self, "swap_logs");
    for i in 0..self.active_log.num_entries {
      self.prev_log.entries[i] = self.active_log.entries[i];
    }
    self.prev_log.num_entries = self.active_log.num_entries;
    self.active_log.num_entries = 0;
  }

  fn synchronize(&mut self) {
    log!(self, "synchronize");
    let global = unsafe { &*self.global };
    let num_threads = global.num_threads.load(Ordering::SeqCst);
    let run_counts: Vec<usize> = (0..num_threads)
      .map(|i| global.threads[i].run_counter)
      .collect();

    for i in 0..num_threads {
      if i == self.thread_id {
        continue;
      }
      loop {
        let thread = global.threads[i];
        if run_counts[i] % 2 == 0
          || thread.run_counter != run_counts[i]
          || self.write_clock <= thread.local_clock
        {
          break;
        }
      }
    }
  }

  fn abort(&mut self) {
    log!(self, "abort");
    self.run_counter += 1;
    if self.is_writer {
      self.unlock_write_log();
    }
  }
}
