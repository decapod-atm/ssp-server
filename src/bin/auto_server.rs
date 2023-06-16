use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::{thread, time};

extern crate ssp_server;

fn main() -> ssp::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .format_timestamp(Some(env_logger::fmt::TimestampPrecision::Millis))
        .init();

    let stop_polling = Arc::new(AtomicBool::new(false));

    // Set signal handlers
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop_polling))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop_polling))?;

    let server = ssp_server::Server::new_auto(
        "/dev/ttyUSB0",
        Arc::clone(&stop_polling),
        ssp::ProtocolVersion::Eight,
    )?;

    let handle = server.handle()?;

    let rgb = [
        // Red
        ssp::RGB::from([0xff, 0, 0]),
        // Green
        ssp::RGB::from([0, 0xff, 0]),
        // Blue
        ssp::RGB::from([0, 0, 0xff]),
        // Orange
        ssp::RGB::from([0xf2, 0xa9, 0]),
        // Purple
        ssp::RGB::from([0x80, 0, 0x80]),
        // Yellow
        ssp::RGB::from([0, 0x80, 0x80]),
    ];
    let mut rgb_idx = 0;
    let rgb_store = ssp::BezelConfigStorage::Ram;

    // do other stuff with the server...
    while !stop_polling.load(Ordering::Relaxed) {
        // Maybe change the bezel color
        handle.configure_bezel(rgb[rgb_idx % rgb.len()], rgb_store)?;
        rgb_idx += 1;
        thread::sleep(time::Duration::from_millis(100));
    }

    handle.disable()?;

    Ok(())
}
