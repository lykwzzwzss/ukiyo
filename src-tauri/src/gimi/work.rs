use std::sync::{atomic::{AtomicBool, Ordering}, Mutex};

pub struct Coordinator {
  closing: AtomicBool,
  lock: Mutex<()>,
}

impl Coordinator {
  pub fn new() -> Self { Self { closing: AtomicBool::new(false), lock: Mutex::new(()) } }
  pub fn run<T>(&self, action: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    if self.closing.load(Ordering::Acquire) { return Err("启动器正在退出".into()); }
    let _guard = self.lock.lock().map_err(|_| "模组工作区状态已损坏".to_string())?;
    if self.closing.load(Ordering::Acquire) { return Err("启动器正在退出".into()); }
    action()
  }
  pub fn shutdown(&self, cleanup: impl FnOnce()) {
    self.closing.store(true, Ordering::Release);
    // Finish the current atomic configuration update, discard queued tasks,
    // then release owned hooks/threads. Nothing may start again after this.
    let _guard = self.lock.lock().unwrap_or_else(|error| error.into_inner());
    cleanup();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::{Arc, Barrier};
  #[test]
  fn closing_waits_for_current_work_and_rejects_queued_or_new_work() {
    let coordinator = Arc::new(Coordinator::new());
    let active = Arc::new(Barrier::new(2));
    let finish = Arc::new(Barrier::new(2));
    let (work, entered, leave) = (coordinator.clone(), active.clone(), finish.clone());
    let worker = std::thread::spawn(move || work.run(|| { entered.wait(); leave.wait(); Ok(()) }));
    active.wait();
    let work = coordinator.clone();
    let queued = std::thread::spawn(move || work.run(|| -> Result<(), String> { panic!("queued preparation must not restart GIMI") }));
    let work = coordinator.clone();
    let cleaned = Arc::new(AtomicBool::new(false));
    let flag = cleaned.clone();
    let shutdown = std::thread::spawn(move || work.shutdown(|| { flag.store(true, Ordering::Release); }));
    while !coordinator.closing.load(Ordering::Acquire) { std::thread::yield_now(); }
    assert!(!cleaned.load(Ordering::Acquire));
    finish.wait();
    worker.join().unwrap().unwrap();
    assert!(queued.join().unwrap().is_err());
    shutdown.join().unwrap();
    assert!(cleaned.load(Ordering::Acquire));
    assert!(coordinator.run(|| -> Result<(), String> { panic!("new task after shutdown") }).is_err());
    coordinator.shutdown(|| {});
  }
}
