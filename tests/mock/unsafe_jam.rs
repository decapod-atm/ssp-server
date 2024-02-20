use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::{process, thread, time};

use ssp::{Error, MessageType, ResponseStatus, Result};

use crate::common;

const MOCK_DEV_PATH: &str = "/tmp/ttyV2";
const MOCK_HOST_PATH: &str = "/tmp/ttyV3";

#[test]
fn test_unsafe_jam() -> Result<()> {
    let _lock = common::init();

    thread::spawn(move || -> Result<()> {
        // Start socat to create virtual PTY devices
        process::Command::new("socat")
            .arg(format!("pty,link={MOCK_DEV_PATH},raw,echo=0").as_str())
            .arg(format!("pty,link={MOCK_HOST_PATH},raw,echo=0,waitslave").as_str())
            .output()?;

        Ok(())
    });

    // Wait for socat to start
    thread::sleep(time::Duration::from_secs(1));

    let stop = Arc::new(AtomicBool::new(false));
    let stop_dev = Arc::new(AtomicBool::new(false));
    let stop_dev_inner = Arc::clone(&stop_dev);

    let mock_handle = thread::spawn(move || -> Result<()> {
        let mut mock = ssp_server::mock::MockDevice::new(
            MOCK_DEV_PATH,
            MessageType::Poll,
            ResponseStatus::UnsafeJam,
        )?;
        mock.serve(stop_dev_inner)
    });

    let handle = ssp_server::DeviceHandle::new(MOCK_HOST_PATH)?;

    handle.start_background_polling(Arc::clone(&stop))?;

    // Wait long enough for multiple poll messages
    thread::sleep(time::Duration::from_secs(3));

    stop_dev.store(true, Ordering::SeqCst);

    mock_handle
        .join()
        .map_err(|err| Error::Io(format!("error joining mock device thread: {err:?}")))??;

    log::trace!("Resetting mock device...");

    // Wait long enough for device to "reset"
    thread::sleep(time::Duration::from_secs(15));

    stop_dev.store(false, Ordering::SeqCst);
    let stop_dev_inner = Arc::clone(&stop_dev);

    let mock_handle = thread::spawn(move || -> Result<()> {
        let mut mock = ssp_server::mock::MockDevice::new(
            MOCK_DEV_PATH,
            MessageType::Poll,
            ResponseStatus::Ok,
        )?;
        mock.serve(stop_dev_inner)
    });

    // Wait long enough for multiple poll messages
    thread::sleep(time::Duration::from_secs(1));

    stop.store(true, Ordering::SeqCst);
    stop_dev.store(true, Ordering::SeqCst);

    mock_handle
        .join()
        .map_err(|err| Error::Io(format!("error joining mock device thread: {err:?}")))??;

    thread::sleep(time::Duration::from_secs(1));

    Ok(())
}
