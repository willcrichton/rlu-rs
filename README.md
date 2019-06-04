# rlu-rs: Read-Log-Update in Rust

This library is an implementation of the Read-Log-Update lock-free concurrency mechanism in Rust. See the [SOSP'15 paper](http://sigops.org/s/conferences/sosp/2015/current/2015-Monterey/printable/077-matveev.pdf) and the [Morning Paper summary](https://blog.acolyer.org/2015/10/27/read-log-update-a-lightweight-synchronization-mechanism-for-concurrent-programming/) for details on the algorithm.

```rust
use std::sync::Arc;
use rlu::{Rlu, RluObject};
use std::{thread, time};

fn main() {
  let rlu: Arc<Rlu<u64>> = Arc::new(Rlu::new());
  let obj: RluObject<u64> = rlu.alloc(1);

  let reader = {
    let rlu = rlu.clone();
    thread::spawn(move || {
      let thread = rlu.thread();
      let mut session = thread.session();
      let n: *const u64 = session.read_lock(obj);
      let n2 = unsafe { *n };
      thread::sleep(time::Duration::from_millis(100));
      assert_eq!(unsafe { *n }, n2);
    })
  };

  let writer = {
    let rlu = rlu.clone();
    thread::spawn(move || {
      let thread = rlu.thread();
      loop {
        let mut session = thread.session();
        match session.write_lock(obj) {
          Some(n) => {
            unsafe { *n += 1; }
            break;
          },
          None => {
            session.abort();
          }
        }
      }
    })
  };

  reader.join().unwrap();
  writer.join().unwrap();
}
```
