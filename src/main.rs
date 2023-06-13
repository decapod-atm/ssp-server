use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

extern crate ssp_server;

use ssp_server::device_handle::*;

fn main() -> ssp::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .format_timestamp(Some(env_logger::fmt::TimestampPrecision::Millis))
        .init();

    let stop_polling = Arc::new(AtomicBool::new(false));

    // Set signal handlers
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop_polling))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop_polling))?;

    let mut handle = DeviceHandle::new("/dev/ttyUSB0")?;

    handle.host_protocol_version(ssp::ProtocolVersion::Eight)?;

    match handle.setup_request() {
        Ok(setup) => log::info!("Device status: {setup}"),
        Err(err) => log::error!("Error retrieving device status: {err}"),
    }

    match handle.serial_number() {
        Ok(sn) => log::info!("Device serial number: {sn}"),
        Err(err) => log::error!("Error retrieving device status: {err}"),
    }

    let rx = handle
        .start_background_polling_with_queue(Arc::clone(&stop_polling), PollMode::Interactive)?;

    let enable_list = ssp::EnableBitfieldList::from([
        ssp::EnableBitfield::from(0xff),
        ssp::EnableBitfield::from(0xff),
        ssp::EnableBitfield::from(0xff),
    ]);

    match handle.set_inhibits(enable_list) {
        Ok(res) => {
            log::debug!("Set inhibits command for 24 channels succeeded: {res}");
        }
        Err(err) => {
            log::error!("Failed set inhibits command for 24 channels: {err}");
        }
    }

    handle.enable()?;

    // do other stuff with the server...
    while !stop_polling.load(Ordering::Relaxed) {
        while let Ok(event) = rx.pop_event() {
            log::info!("Received event: {event}");

            // Handle the event...
        }
    }

    handle.disable()?;

    // stop the background polling routine
    stop_polling.store(true, Ordering::SeqCst);

    Ok(())
}
