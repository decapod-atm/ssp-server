use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::{process, thread, time};

use ssp::{Error, MessageType, ResponseStatus, Result};

use crate::common;

use super::{MOCK_DEV_PATH, MOCK_HOST_PATH};

#[test]
fn test_poll() -> Result<()> {
    let _lock = common::init();

    thread::spawn(move || -> Result<()> {
        // Start socat to create virtual PTY devices
        process::Command::new("socat")
            .arg(format!("pty,link={MOCK_DEV_PATH},raw,echo=0").as_str())
            .arg(format!("pty,link={MOCK_HOST_PATH},raw,echo=0,waitslave").as_str())
            .output()?;

        Ok(())
    });

    thread::sleep(time::Duration::from_secs(1));

    let stop = Arc::new(AtomicBool::new(false));
    let stop_device = Arc::clone(&stop);

    let mock_handle = thread::spawn(move || -> Result<()> {
        let mut mock = ssp_server::mock::MockDevice::new(
            MOCK_DEV_PATH,
            MessageType::Poll,
            ResponseStatus::Ok,
        )?;
        mock.serve(stop_device)
    });

    let handle = ssp_server::DeviceHandle::new(MOCK_HOST_PATH)?;

    handle.start_background_polling(Arc::clone(&stop))?;

    thread::sleep(time::Duration::from_secs(1));

    stop.store(true, Ordering::SeqCst);

    mock_handle
        .join()
        .map_err(|err| Error::Io(format!("error joining mock device thread: {err:?}")))??;

    Ok(())
}
