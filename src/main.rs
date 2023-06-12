use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

mod device_handle;
use device_handle::*;

fn main() -> ssp::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .format_timestamp(Some(env_logger::fmt::TimestampPrecision::Millis))
        .init();

    let stop_polling = Arc::new(AtomicBool::new(false));

    let mut handle = DeviceHandle::new("/dev/ttyUSB0")?;

    handle.start_background_polling(Arc::clone(&stop_polling))?;

    handle.enable()?;

    // do other stuff with the server...

    handle.disable()?;

    // stop the background polling routine
    stop_polling.store(true, Ordering::SeqCst);

    Ok(())
}
