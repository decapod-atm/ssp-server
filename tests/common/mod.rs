#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::time;

use parking_lot::{Mutex, MutexGuard};

use ssp::{Error, Result};

static INIT: AtomicBool = AtomicBool::new(false);
static LOCK: Mutex<()> = Mutex::new(());

fn is_init() -> bool {
    INIT.load(Ordering::Relaxed)
}

fn set_init(val: bool) {
    INIT.store(val, Ordering::SeqCst);
}

pub fn init() -> Result<MutexGuard<'static, ()>> {
    if !is_init() {
        set_init(true);
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
            .format_timestamp(Some(env_logger::fmt::TimestampPrecision::Millis))
            .try_init()
            .ok();
    }

    LOCK.try_lock_for(time::Duration::from_secs(5))
        .ok_or(Error::Io("lock test mutex".into()))
}
