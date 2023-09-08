use std::os::fd::RawFd;
use std::path::Path;
use std::sync::mpsc::channel;
use std::{collections::HashMap, path::PathBuf, sync::Mutex};

use nix::{ioctl_write_int_bad, request_code_write};
use threadpool::ThreadPool;

// https://github.com/torvalds/linux/blob/68e77ffbfd06ae3ef8f2abf1c3b971383c866983/include/uapi/linux/input.h#L186
ioctl_write_int_bad!(eviocgrab, request_code_write!('E', 0x90, 4));

lazy_static::lazy_static! {
    static ref DEVICES: Mutex<HashMap<PathBuf, RawFd>> = Mutex::new(HashMap::new());
    static ref THREAD_POOL: SyncThreadPool = SyncThreadPool::new(4);
}

pub fn add_device(path: &Path, fd: RawFd) {
    let mut devices = DEVICES.lock().unwrap();
    devices.insert(path.to_owned(), fd);
}

pub fn remove_device(fd: RawFd) {
    let mut devices = DEVICES.lock().unwrap();
    devices.retain(|_, &mut v| v != fd);
}

pub fn grab_devices(grab: bool) {
    let devices = DEVICES.lock().unwrap();

    let (tx, rx) = channel();

    for (path, device) in devices.iter() {
        let device = *device;
        let path = path.clone();

        let tx = tx.clone();
        THREAD_POOL.execute(move || {
            match unsafe { eviocgrab(device, grab.into()) } {
                Ok(_) => {}
                Err(e) => {
                    log::error!(
                        "Failed to {} {}: {}",
                        if grab { "grab" } else { "ungrab" },
                        path.display(),
                        e
                    );
                }
            }

            let _ = tx.send(());
        });
    }

    drop(tx);
    for _ in rx.iter() {}
}

/// A Send + Sync thread pool.
#[derive(Debug)]
struct SyncThreadPool {
    pool: Mutex<ThreadPool>,
}

impl SyncThreadPool {
    /// Create a new thread pool with the specified size.
    fn new(num_threads: usize) -> Self {
        Self {
            pool: Mutex::new(ThreadPool::new(num_threads)),
        }
    }

    /// Execute a job on the thread pool.
    fn execute(&self, job: impl FnOnce() + Send + 'static) {
        self.pool
            .lock()
            .expect("could not lock thread pool mutex")
            .execute(job)
    }
}
