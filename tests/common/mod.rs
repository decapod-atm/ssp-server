#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};

static IS_INIT: AtomicBool = AtomicBool::new(false);

fn is_init() -> bool {
    IS_INIT.load(Ordering::Relaxed)
}

fn set_is_init() -> bool {
    IS_INIT.store(true, Ordering::SeqCst);
    IS_INIT.load(Ordering::Relaxed)
}

pub fn init() {
    if !is_init() {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
            .format_timestamp(Some(env_logger::fmt::TimestampPrecision::Millis))
            .init();

        set_is_init();
    }
}
