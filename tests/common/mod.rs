#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::time;

use parking_lot::{Mutex, MutexGuard};

use ssp::{Error, Result};

static IS_INIT: AtomicBool = AtomicBool::new(false);
static LOCK: Mutex<()> = Mutex::new(());

fn is_init() -> bool {
    IS_INIT.load(Ordering::Relaxed)
}

fn set_is_init() -> bool {
    IS_INIT.store(true, Ordering::SeqCst);
    IS_INIT.load(Ordering::Relaxed)
}

pub fn init() -> Result<MutexGuard<'static, ()>> {
    if !is_init() {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
            .format_timestamp(Some(env_logger::fmt::TimestampPrecision::Millis))
            .init();

        set_is_init();
    }

    LOCK.try_lock_for(time::Duration::from_secs(5))
        .ok_or(Error::Io("lock test mutex".into()))
}
