#![allow(unused_imports, dead_code, unused_variables)]

use std::sync::atomic::{compiler_fence, Ordering, AtomicUsize};
use std::usize;
use std::ptr;
use std::sync::Arc;
use std::cell::RefCell;
use std::mem::transmute;
use std::fmt::Debug;

const RLU_MAX_LOG_SIZE: usize = 32;
const RLU_MAX_THREADS: usize = 32;

#[derive(Default, Clone, Copy)]
struct ObjOriginal<T> {
  copy: Option<*mut ObjCopy<T>>,
  data: T
}

#[derive(Clone, Copy)]
struct ObjCopy<T> {
  thread_id: usize,
  original: *mut ObjOriginal<T>,
  data: T
}

unsafe impl<T> Send for ObjOriginal<T> {}
unsafe impl<T> Sync for ObjOriginal<T> {}

unsafe impl<T> Send for ObjCopy<T> {}
unsafe impl<T> Sync for ObjCopy<T> {}

#[derive(Clone, Copy)]
enum RluObject<T> {
  Original(ObjOriginal<T>),
  Copy(ObjCopy<T>)
}

impl<T: Default> Default for ObjCopy<T> {
  fn default() -> Self {
    ObjCopy {
      thread_id: 0, data: T::default(), original: ptr::null_mut()
    }
  }
}

#[derive(Default, Clone, Copy)]
struct WriteLog<T> {
  entries: [ObjCopy<T>; RLU_MAX_LOG_SIZE],
  num_entries: usize
}

#[derive(Clone, Copy)]
struct RluThread<T> {
  active_log: WriteLog<T>,
  prev_log: WriteLog<T>,
  is_writer: bool,
  write_clock: usize,
  local_clock: usize,
  run_counter: usize,
  thread_id: usize
}

struct Rlu<T> {
  global_clock: AtomicUsize,
  threads: [RluThread<T>; RLU_MAX_THREADS],
  num_threads: AtomicUsize
}

trait ObjectBounds: Default + Copy + Debug {}
impl<T: Default + Copy + Debug> ObjectBounds for T {}

impl<T: ObjectBounds> RluObject<T> {
  fn new(data: T) -> RluObject<T> {
    RluObject::Original(ObjOriginal {
      copy: None,
      data
    })
  }
}

impl<T: ObjectBounds> WriteLog<T> {
  fn next_entry(&mut self) -> &mut ObjCopy<T> {
    let i = self.num_entries;
    self.num_entries += 1;
    &mut self.entries[i]
  }
}

impl<T: ObjectBounds> Rlu<T> {
  fn new() -> Rlu<T> {
    Rlu {
      global_clock: AtomicUsize::new(0),
      threads: [RluThread::new(); RLU_MAX_THREADS],
      num_threads: AtomicUsize::new(0)
    }
  }

  fn make_thread(&self) -> &mut RluThread<T> {
    let thread_id = self.num_threads.fetch_add(1, Ordering::SeqCst);
    let thread: *mut RluThread<T> =
      &self.threads[thread_id] as *const RluThread<T> as *mut RluThread<T>;
    let thread: &mut RluThread<T> = unsafe { &mut *thread };
    thread.thread_id = thread_id;
    return thread;
  }

  fn get_thread(&self, index: usize) -> *mut RluThread<T> {
    unsafe { transmute(&self.threads[index] as *const RluThread<T>) }
  }
}

macro_rules! log {
  ($self:expr, $e:expr) => {
    let s: String = $e.into();
    println!("Thread {}: {}", $self.thread_id, s);
  }
}

impl<T: ObjectBounds> RluThread<T> {
  fn new() -> RluThread<T> {
    RluThread {
      active_log: WriteLog::default(),
      prev_log: WriteLog::default(),
      is_writer: false,
      write_clock: usize::MAX,
      local_clock: 0,
      run_counter: 0,
      thread_id: 0
    }
  }

  fn reader_lock(&mut self, global: &Arc<Rlu<T>>) {
    log!(self, "reader_lock");
    self.run_counter += 1;
    self.local_clock = global.global_clock.load(Ordering::SeqCst);
    self.is_writer = false;
  }

  fn writeback_logs(&mut self) {
    log!(self, "writeback_logs");
    for i in 0 .. self.active_log.num_entries {
      let copy = &mut self.active_log.entries[i];
      log!(self, format!("copy {:?}", copy.data));
      unsafe { (*copy.original).data = copy.data; }
    }
  }

  fn unlock_write_log(&mut self) {
    log!(self, "unlock_write_log");
    for i in 0 .. self.active_log.num_entries {
      unsafe { (*self.active_log.entries[i].original).copy = None; }
    }
  }

  fn swap_logs(&mut self) {
    log!(self, "swap_logs");
    for i in 0 .. self.active_log.num_entries {
      self.prev_log.entries[i] = self.active_log.entries[i];
    }
    self.prev_log.num_entries = self.active_log.num_entries;
    self.active_log.num_entries = 0;
  }

  fn reader_unlock(&mut self, global: &Arc<Rlu<T>>) {
    log!(self, "reader_unlock");
    self.run_counter += 1;

    if self.is_writer {
      self.write_clock = global.global_clock.fetch_add(1, Ordering::SeqCst) + 1;
      self.synchronize(global);
      self.writeback_logs();
      self.unlock_write_log();
      self.write_clock = usize::MAX;
      self.swap_logs();
    }
  }

  fn dereference<'a>(
    &mut self,
    global: &Arc<Rlu<T>>,
    obj: &'a mut RluObject<T>)
    -> &'a mut T
  {
    log!(self, "dereference");
    match obj {
      RluObject::Copy(ref mut copy) => &mut copy.data,
      RluObject::Original(ref mut orig) => {
        match orig.copy {
          None => &mut orig.data,
          Some(copy) => {
            let copy = unsafe { &mut *copy };
            if self.thread_id == copy.thread_id {
              &mut copy.data
            } else {
              let thread = unsafe { &mut *global.get_thread(copy.thread_id) };
              if thread.write_clock <= self.local_clock {
                &mut copy.data
              } else {
                &mut orig.data
              }
            }
          }
        }
      }
    }
  }

  fn try_lock(
    &mut self,
    global: &Arc<Rlu<T>>,
    obj: &mut RluObject<T>)
    -> Option<&mut T>
  {
    self.is_writer = true;
    let orig = match obj {
      RluObject::Original(ref mut orig) => {
        match orig.copy {
          Some(copy) => {
            let copy = unsafe { &mut *copy };
            if self.thread_id == copy.thread_id {
              return Some(&mut copy.data);
            } else {
              self.abort();
              return None;
            }
          },
          None => orig
        }
      },

      RluObject::Copy(copy) => unsafe { &mut *copy.original }
    };

    let copy = self.active_log.next_entry();
    copy.thread_id = self.thread_id;
    copy.original = orig as *mut ObjOriginal<T>;
    copy.data = orig.data;

    orig.copy = Some(copy as *mut ObjCopy<T>);

    Some(&mut copy.data)
  }

  fn synchronize(&mut self, global: &Arc<Rlu<T>>) {
    log!(self, "synchronize");
    let num_threads = global.num_threads.load(Ordering::SeqCst);
    let run_counts: Vec<usize> =
      (0 .. num_threads).map(|i| global.threads[i].run_counter).collect();

    for i in 0 .. num_threads {
      if i == self.thread_id { continue; }
      loop {
        let thread = global.threads[i];
        // log!(self, format!(
        //   "sync on {}: {}, {}, {}",
        //   i, run_counts[i] % 2 == 0, thread.run_counter != run_counts[i],
        // self.write_clock <= thread.local_clock));
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

#[cfg(test)]
mod tests {
  #![allow(unused_mut, unused_variables)]
  use super::*;
  use std::thread;
  use std::sync::mpsc;

  #[test]
  fn basic() {
    let rlu: Arc<Rlu<u64>> = Arc::new(Rlu::new());
    let mut obj = RluObject::new(3);
    let thread = rlu.make_thread();
    thread.reader_lock(&rlu);

    {
      let n = thread.dereference(&rlu, &mut obj);
      assert_eq!(*n, 3);
    }

    {
      let n = thread.try_lock(&rlu, &mut obj).unwrap();
      assert_eq!(*n, 3);
      *n += 1;
    }

    {
      let n = thread.dereference(&rlu, &mut obj);
      assert_eq!(*n, 4);
    }

    thread.reader_unlock(&rlu);

    thread.reader_lock(&rlu);

    {
      let n = thread.dereference(&rlu, &mut obj);
      assert_eq!(*n, 4);
    }

    thread.reader_unlock(&rlu);
  }

  #[test]
  fn concurrent_reader_writer() {
    let rlu: Arc<Rlu<u64>> = Arc::new(Rlu::new());
    let mut obj1 = RluObject::new(3);
    let mut obj2 = RluObject::new(5);

    let thread0 = rlu.make_thread();
    thread0.reader_lock(&rlu);

    let thread1 = rlu.make_thread();
    thread1.reader_lock(&rlu);

    let n3 = {
      let n1: &mut u64 = thread1.dereference(&rlu, &mut obj1);
      assert_eq!(*n1, 3);
      n1 as *mut u64
    };

    {
      let n2: &mut u64 = thread0.try_lock(&rlu, &mut obj1).unwrap();
      assert_eq!(*n2, 3);
      *n2 += 1;
    }

    assert_eq!(unsafe { *n3 }, 3);

    thread1.reader_unlock(&rlu);
    thread0.reader_unlock(&rlu);

    assert_eq!(unsafe { *n3 }, 4);
  }

  #[test]
  fn thread() {
    let rlu: Arc<Rlu<u64>> = Arc::new(Rlu::new());
    let mut obj1 = RluObject::new(3);
    let mut obj2 = RluObject::new(5);

    let rlu1 = rlu.clone();
    let t0 = thread::spawn(move || {
      let thread0 = rlu1.make_thread();
      thread0.reader_lock(&rlu1);

      let n2: &mut u64 = thread0.try_lock(&rlu, &mut obj1).unwrap();
      assert_eq!(*n2, 3);
      *n2 += 1;

      thread0.reader_unlock(&rlu);
    });
  }
}
