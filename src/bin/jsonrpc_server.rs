use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

extern crate ssp_server;

use ssp::jsonrpc;

fn main() -> ssp::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .format_timestamp(Some(env_logger::fmt::TimestampPrecision::Millis))
        .init();

    let stop = Arc::new(AtomicBool::new(false));

    // Set signal handlers
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop))?;

    let socket_path =
        jsonrpc::get_socket_path(jsonrpc::JSONRPC_ENV_SOCK, jsonrpc::JSONRPC_SOCKET_PATH);
    let mut server = ssp_server::Server::new_uds(
        "/dev/ttyUSB0",
        socket_path.as_str(),
        Arc::clone(&stop),
        ssp::ProtocolVersion::Eight,
        false,
    )?;

    while !stop.load(Ordering::Relaxed) {
        server.accept(Arc::clone(&stop))?;
    }

    // stop the background polling routine
    stop.store(true, Ordering::SeqCst);

    Ok(())
}
