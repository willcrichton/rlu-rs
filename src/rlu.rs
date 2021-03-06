#![allow(dead_code, unused_variables)]

use std::fmt::Debug;
use std::mem;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::thread;
use std::usize;

const RLU_MAX_LOG_SIZE: usize = 128;
const RLU_MAX_THREADS: usize = 32;
const RLU_MAX_FREE_NODES: usize = 100;

pub struct ObjOriginal<T> {
  copy: AtomicPtr<ObjCopy<T>>,
  data: T,
}

pub struct ObjCopy<T> {
  thread_id: usize,
  original: RluObject<T>,
  data: T,
}

#[derive(Debug)]
pub struct RluObject<T>(pub(crate) *mut ObjOriginal<T>);

impl<T> Clone for RluObject<T> {
  fn clone(&self) -> Self {
    *self
  }
}
impl<T> Copy for RluObject<T> {}

impl<T> RluObject<T> {
  fn deref(&self) -> &ObjOriginal<T> {
    unsafe { &*self.0 }
  }

  fn deref_mut(&mut self) -> &mut ObjOriginal<T> {
    unsafe { &mut *self.0 }
  }
}

struct WriteLog<T> {
  entries: [ObjCopy<T>; RLU_MAX_LOG_SIZE],
  num_entries: usize,
}

pub struct RluThread<T> {
  logs: [WriteLog<T>; 2],
  current_log: usize,
  is_writer: bool,
  write_clock: usize,
  local_clock: AtomicUsize,
  run_counter: AtomicUsize,
  thread_id: usize,
  global: *const Rlu<T>,
  free_list: [RluObject<T>; RLU_MAX_FREE_NODES],
  num_free: usize,
}

pub struct Rlu<T> {
  global_clock: AtomicUsize,
  threads: [RluThread<T>; RLU_MAX_THREADS],
  num_threads: AtomicUsize,
}

unsafe impl<T> Send for RluObject<T> {}
unsafe impl<T> Sync for RluObject<T> {}

unsafe impl<T> Send for RluThread<T> {}
unsafe impl<T> Sync for RluThread<T> {}

pub struct RluSession<'a, T: RluBounds> {
  t: &'a mut RluThread<T>,
  abort: bool,
}

pub trait RluBounds: Clone + Debug {}
impl<T: Clone + Debug> RluBounds for T {}

impl<T> WriteLog<T> {
  fn next_entry(&mut self) -> &mut ObjCopy<T> {
    let i = self.num_entries;
    self.num_entries += 1;

    if cfg!(debug_assertions) {
      assert!(self.num_entries < RLU_MAX_LOG_SIZE)
    }

    unsafe { self.entries.get_unchecked_mut(i) }
  }
}

impl<T: RluBounds> Rlu<T> {
  pub fn new() -> Rlu<T> {
    Rlu {
      global_clock: AtomicUsize::new(0),
      num_threads: AtomicUsize::new(0),
      threads: unsafe { mem::uninitialized() },
    }
  }

  pub fn thread(&self) -> &mut RluThread<T> {
    let thread_id = self.num_threads.fetch_add(1, Ordering::SeqCst);
    let thread: *mut RluThread<T> =
      &self.threads[thread_id] as *const RluThread<T> as *mut RluThread<T>;
    let thread: &mut RluThread<T> = unsafe { &mut *thread };
    *thread = RluThread::new();
    thread.thread_id = thread_id;
    thread.global = self as *const Rlu<T>;
    return thread;
  }

  fn get_thread(&self, index: usize) -> *mut RluThread<T> {
    (unsafe { self.threads.get_unchecked(index) }) as *const RluThread<T>
      as *mut RluThread<T>
  }

  pub fn alloc(&self, data: T) -> RluObject<T> {
    RluObject(Box::into_raw(Box::new(ObjOriginal {
      copy: AtomicPtr::new(ptr::null_mut()),
      data,
    })))
  }
}

macro_rules! log {
  ($self:expr, $e:expr) => {
    if cfg!(debug_assertions) {
      let s: String = $e.into();
      println!("Thread {}: {}", $self.thread_id, s);
    }
  };
}

impl<'a, T: RluBounds> RluSession<'a, T> {
  pub fn read_lock(&mut self, obj: RluObject<T>) -> *const T {
    log!(self.t, "dereference");
    let global = unsafe { &*self.t.global };
    let orig = obj.deref();
    match unsafe { orig.copy.load(Ordering::SeqCst).as_ref() } {
      None => &orig.data,
      Some(copy) => {
        if self.t.thread_id == copy.thread_id {
          log!(
            self.t,
            format!("dereference self copy {:?} ({:p})", copy.data, &copy.data)
          );
          &copy.data
        } else {
          let thread = unsafe { &*global.get_thread(copy.thread_id) };
          if thread.write_clock <= self.t.local_clock.load(Ordering::SeqCst) {
            log!(self.t,
                 format!("dereference other copy {:?} ({:p}), write clock {}, local clock {}", copy.data, &copy.data, thread.write_clock, self.t.local_clock.load(Ordering::SeqCst)));
            &copy.data
          } else {
            log!(
              self.t,
              format!(
                "dereferencing original {:?} ({:p})",
                orig.data, &orig.data
              )
            );
            &orig.data
          }
        }
      }
    }
  }

  pub fn write_lock(&mut self, mut obj: RluObject<T>) -> Option<*mut T> {
    log!(self.t, format!("try_lock"));
    let global = unsafe { &*self.t.global };
    self.t.is_writer = true;

    if let Some(copy) =
      unsafe { obj.deref_mut().copy.load(Ordering::SeqCst).as_mut() }
    {
      if self.t.thread_id == copy.thread_id {
        log!(
          self.t,
          format!("locked existing copy {:?} ({:p})", copy.data, &copy.data)
        );
        return Some(&mut copy.data as *mut T);
      } else {
        return None;
      }
    }

    let active_log = &mut self.t.logs[self.t.current_log];
    let copy = active_log.next_entry();
    copy.thread_id = self.t.thread_id;
    copy.data = obj.deref().data.clone();
    copy.original = obj;
    let prev_ptr = obj.deref_mut().copy.compare_and_swap(
      ptr::null_mut(),
      copy,
      Ordering::SeqCst,
    );
    if prev_ptr != ptr::null_mut() {
      active_log.num_entries -= 1;
      return None;
    }

    log!(
      self.t,
      format!("locked new copy {:?} ({:p})", copy.data, &copy.data)
    );

    Some(&mut copy.data as *mut T)
  }

  pub fn abort(mut self) {
    self.abort = true;
  }
}

impl<'a, T: RluBounds> Drop for RluSession<'a, T> {
  fn drop(&mut self) {
    log!(self.t, "drop");
    if self.abort {
      self.t.abort();
    } else {
      self.t.unlock();
    }
  }
}

impl<T: RluBounds> RluThread<T> {
  fn new() -> RluThread<T> {
    let mut thread = RluThread {
      logs: unsafe { mem::uninitialized() },
      current_log: 0,
      is_writer: false,
      write_clock: usize::MAX,
      local_clock: AtomicUsize::new(0),
      run_counter: AtomicUsize::new(0),
      thread_id: 0,
      global: ptr::null(),
      num_free: 0,
      free_list: unsafe { mem::uninitialized() },
    };

    for i in 0..2 {
      thread.logs[i].num_entries = 0;
    }

    thread
  }

  pub fn session<'a>(&'a mut self) -> RluSession<'a, T> {
    log!(self, "lock");
    let global = unsafe { &*self.global };
    let cntr = self.run_counter.fetch_add(1, Ordering::SeqCst);
    if cfg!(debug_assertions) {
      assert!(cntr % 2 == 0);
    }

    self
      .local_clock
      .store(global.global_clock.load(Ordering::SeqCst), Ordering::SeqCst);
    log!(
      self,
      format!(
        "lock with local clock {}",
        self.local_clock.load(Ordering::SeqCst)
      )
    );
    self.is_writer = false;
    RluSession {
      t: self,
      abort: false,
    }
  }

  pub fn free(&mut self, obj: RluObject<T>) {
    let free_id = self.num_free;
    self.num_free += 1;
    self.free_list[free_id] = obj;
  }

  fn process_free(&mut self) {
    for i in 0..self.num_free {
      unsafe { Box::from_raw(self.free_list[i].0) };
    }

    self.num_free = 0;
  }

  fn commit_write_log(&mut self) {
    let global = unsafe { &*self.global };
    self.write_clock = global.global_clock.fetch_add(1, Ordering::SeqCst) + 1;
    log!(self, format!("global clock: {}", self.write_clock));
    self.synchronize();
    self.writeback_logs();
    self.unlock_write_log();
    self.write_clock = usize::MAX;
    self.swap_logs();
    self.process_free();
  }

  fn unlock(&mut self) {
    log!(self, "unlock");
    let cntr = self.run_counter.fetch_add(1, Ordering::SeqCst);
    if cfg!(debug_assertions) {
      assert!(cntr % 2 == 1);
    }

    if self.is_writer {
      self.commit_write_log();
    }
  }

  fn writeback_logs(&mut self) {
    log!(self, "writeback_logs");
    let active_log = &mut self.logs[self.current_log];
    for i in 0..active_log.num_entries {
      let copy = &mut active_log.entries[i];
      log!(self, format!("copy {:?} ({:p})", copy.data, &copy.data));
      let orig = copy.original.deref_mut();
      orig.data = copy.data.clone();
    }
  }

  fn unlock_write_log(&mut self) {
    log!(self, "unlock_write_log");
    let active_log = &mut self.logs[self.current_log];
    for i in 0..active_log.num_entries {
      let orig = active_log.entries[i].original.deref_mut();
      orig.copy.store(ptr::null_mut(), Ordering::SeqCst);
    }
    active_log.num_entries = 0;
  }

  fn swap_logs(&mut self) {
    log!(self, "swap_logs");
    self.current_log = (self.current_log + 1) % 2;
    let active_log = &mut self.logs[self.current_log];
    active_log.num_entries = 0;
  }

  fn synchronize(&mut self) {
    log!(self, "synchronize");

    let global = unsafe { &*self.global };
    let num_threads = global.num_threads.load(Ordering::SeqCst);
    let run_counts: Vec<usize> = (0..num_threads)
      .map(|i| global.threads[i].run_counter.load(Ordering::SeqCst))
      .collect();

    for i in 0..num_threads {
      if i == self.thread_id {
        continue;
      }

      let thread = &global.threads[i];
      loop {
        log!(self, format!("wait on thread {}: rc {}, counter {}, write clock {}, local clock {}", i, run_counts[i], thread.run_counter.load(Ordering::SeqCst), self.write_clock, thread.local_clock.load(Ordering::SeqCst)));

        if run_counts[i] % 2 == 0
          || thread.run_counter.load(Ordering::SeqCst) != run_counts[i]
          || self.write_clock <= thread.local_clock.load(Ordering::SeqCst)
        {
          break;
        }

        thread::yield_now();
      }
    }
  }

  fn abort(&mut self) {
    log!(self, "abort");
    let cntr = self.run_counter.fetch_add(1, Ordering::SeqCst);
    if cfg!(debug_assertions) {
      assert!(cntr % 2 == 1);
    }

    if self.is_writer {
      self.unlock_write_log();
    }
  }
}
