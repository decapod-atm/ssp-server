#[cfg(feature = "jsonrpc")]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(feature = "jsonrpc")]
use std::sync::atomic::Ordering;
use std::sync::{atomic::AtomicBool, Arc};
use std::time;
#[cfg(feature = "jsonrpc")]
use std::{io::Write, net::Shutdown, path::PathBuf, thread};

use parking_lot::{Mutex, MutexGuard};
use ssp::{Error, Result};
#[cfg(feature = "jsonrpc")]
use ssp::{Event, Method};

use crate::DeviceHandle;
#[cfg(feature = "jsonrpc")]
use crate::{continue_on_err, PollMode, PushEventReceiver};

const HANDLE_TIMEOUT_MS: u128 = 5_000;
const MAX_RESETS: u64 = 10;

#[cfg(feature = "jsonrpc")]
static STOP_SERVING_CLIENT: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "jsonrpc")]
fn stop_serving_client() -> bool {
    STOP_SERVING_CLIENT.load(Ordering::Relaxed)
}

#[cfg(feature = "jsonrpc")]
fn set_stop_serving_client(s: bool) {
    STOP_SERVING_CLIENT.store(s, Ordering::SeqCst);
}

/// SSP/eSSP server for communicating with supported device over serial.
///
/// Optionally, can be used interactively with a client connection.
pub struct Server {
    pub handle: Arc<Mutex<DeviceHandle>>,
    #[cfg(feature = "jsonrpc")]
    socket_path: Option<String>,
    #[cfg(feature = "jsonrpc")]
    push_queue: Option<PushEventReceiver>,
    #[cfg(feature = "jsonrpc")]
    listener: Option<UnixListener>,
    #[cfg(feature = "jsonrpc")]
    bus: Option<bus::Bus<Event>>,
}

impl Server {
    /// Creates a new [Server] that automatically handles device events.
    pub fn new_auto(
        serial_path: &str,
        stop_polling: Arc<AtomicBool>,
        protocol_version: ssp::ProtocolVersion,
    ) -> Result<Self> {
        let handle = DeviceHandle::new(serial_path)?;
        handle.start_background_polling(stop_polling)?;
        // enable the device to fully configure
        handle.enable_device(protocol_version)?;

        Ok(Self {
            handle: Arc::new(Mutex::new(handle)),
            #[cfg(feature = "jsonrpc")]
            socket_path: None,
            #[cfg(feature = "jsonrpc")]
            push_queue: None,
            #[cfg(feature = "jsonrpc")]
            listener: None,
            #[cfg(feature = "jsonrpc")]
            bus: None,
        })
    }

    /// Creates a new [Server] that interactively handles device events via
    /// a client connects over Unix domain sockets.
    #[cfg(feature = "jsonrpc")]
    #[cfg_attr(doc_cfg, doc(cfg(feature = "jsonrpc")))]
    pub fn new_uds(
        serial_path: &str,
        socket_path: &str,
        stop_polling: Arc<AtomicBool>,
        protocol_version: ssp::ProtocolVersion,
    ) -> Result<Self> {
        let handle = DeviceHandle::new(serial_path)?;
        let push_queue =
            Some(handle.start_background_polling_with_queue(stop_polling, PollMode::Interactive)?);

        // enable the device to fully configure
        let mut reset_count = 0;
        while let Err(err) = handle.enable_device(protocol_version) {
            log::error!("error enabling device: {err}, resetting");

            if let Err(err) = handle.full_reset() {
                log::error!("error resetting device: {err}");
            }

            reset_count += 1;

            if reset_count >= MAX_RESETS {
                log::error!("maximum resets reached: {MAX_RESETS}");
                break;
            }
        }

        // disable again to allow client to decide when to begin accepting notes
        handle.disable()?;

        // ensure path exists for UNIX socket
        let path_dir = PathBuf::from(socket_path);
        let sock_path = path_dir
            .parent()
            .ok_or(Error::Io("creating PathBuf from socket path".into()))?;
        std::fs::create_dir_all(sock_path)?;

        let listener = Some(UnixListener::bind(socket_path)?);
        let bus = Some(bus::Bus::new(1024));

        Ok(Self {
            handle: Arc::new(Mutex::new(handle)),
            socket_path: Some(socket_path.into()),
            push_queue,
            listener,
            bus,
        })
    }

    /// Gets a reference to the [DeviceHandle].
    pub fn handle(&self) -> Result<MutexGuard<'_, DeviceHandle>> {
        Self::lock_handle(&self.handle)
    }

    /// Aquires a lock on the [DeviceHandle].
    ///
    /// Returns an `Err(_)` if the timeout expires before acquiring the lock.
    pub fn lock_handle(handle: &Arc<Mutex<DeviceHandle>>) -> Result<MutexGuard<'_, DeviceHandle>> {
        let now = time::Instant::now();

        while now.elapsed().as_millis() < HANDLE_TIMEOUT_MS {
            if let Some(lock) = handle.try_lock() {
                return Ok(lock);
            }
        }

        Err(Error::Timeout("waiting for DeviceHandle".into()))
    }

    /// Gets a reference to the event queue bus for broadcasting [Event]s.
    ///
    /// Returns `Err(_)` if the [Bus](bus::Bus) is unset.
    #[cfg(feature = "jsonrpc")]
    pub fn bus(&self) -> Result<&bus::Bus<Event>> {
        self.bus
            .as_ref()
            .ok_or(ssp::Error::JsonRpc("unset event queue Bus".into()))
    }

    /// Gets a mutable reference to the event queue bus for broadcasting [Event]s.
    ///
    /// Returns `Err(_)` if the [Bus](bus::Bus) is unset.
    #[cfg(feature = "jsonrpc")]
    pub fn bus_mut(&mut self) -> Result<&mut bus::Bus<Event>> {
        self.bus
            .as_mut()
            .ok_or(ssp::Error::JsonRpc("unset event queue Bus".into()))
    }

    /// Gets a reference to the push event queue.
    ///
    /// Returns `Err(_)` if the [PushEventReceiver](crate::PushEventReceiver) is unset.
    #[cfg(feature = "jsonrpc")]
    pub fn push_queue(&self) -> Result<&PushEventReceiver> {
        self.push_queue
            .as_ref()
            .ok_or(ssp::Error::JsonRpc("unset push event queue".into()))
    }

    /// Gets a mutable reference to the push event queue.
    ///
    /// Returns `Err(_)` if the [PushEventReceiver](crate::PushEventReceiver) is unset.
    #[cfg(feature = "jsonrpc")]
    pub fn push_queue_mut(&mut self) -> Result<&mut PushEventReceiver> {
        self.push_queue
            .as_mut()
            .ok_or(ssp::Error::JsonRpc("unset push event queue".into()))
    }

    /// Gets a reference to the [Listener](UnixListener).
    ///
    /// Returns `Err(_)` if the [Listener](UnixListener) is unset.
    #[cfg(feature = "jsonrpc")]
    pub fn listener(&self) -> Result<&UnixListener> {
        self.listener
            .as_ref()
            .ok_or(ssp::Error::JsonRpc("unset unix listener".into()))
    }

    /// Gets a mutable reference to the push event queue.
    ///
    /// Returns `Err(_)` if the [PushEventReceiver](crate::PushEventReceiver) is unset.
    #[cfg(feature = "jsonrpc")]
    pub fn listener_mut(&mut self) -> Result<&mut UnixListener> {
        self.listener
            .as_mut()
            .ok_or(ssp::Error::JsonRpc("unset unix listener".into()))
    }

    #[cfg(feature = "jsonrpc")]
    pub fn accept(&mut self, stop: Arc<AtomicBool>) -> Result<()> {
        self.listener()?.set_nonblocking(true)?;

        while !stop.load(Ordering::Relaxed) {
            if let Ok((mut stream, _)) = self.listener_mut()?.accept() {
                log::debug!("Accepted new connection");

                let socket_timeout = std::env::var("SSP_SOCKET_TIMEOUT")
                    .unwrap_or("1".into())
                    .parse::<u64>()
                    .unwrap_or(1);

                stream.set_read_timeout(Some(time::Duration::from_secs(socket_timeout)))?;
                stream.set_write_timeout(Some(time::Duration::from_secs(socket_timeout)))?;

                let handle = Arc::clone(&self.handle);
                let stop_stream = Arc::clone(&stop);
                let mut rx = self.bus_mut()?.add_rx();

                thread::spawn(move || -> Result<()> {
                    while !stop_stream.load(Ordering::Relaxed) {
                        let mut lock = continue_on_err!(
                            Self::lock_handle(&handle),
                            "lock handle in accept loop"
                        );
                        match Self::receive(&mut lock, &mut stream) {
                            Ok(method) => match method {
                                Method::Enable | Method::StackerFull => {
                                    set_stop_serving_client(false);
                                }
                                Method::Disable => {
                                    if stop_serving_client() {
                                        let _ = stream.shutdown(Shutdown::Both);
                                        set_stop_serving_client(false);
                                        log::debug!("Shutting down stream");
                                        return Ok(());
                                    } else {
                                        set_stop_serving_client(true);
                                        continue;
                                    }
                                }
                                Method::Shutdown => {
                                    log::debug!("Shutting down the socket connection");
                                    stream.shutdown(Shutdown::Both)?;
                                    return Ok(());
                                }
                                _ => log::debug!("Handled method: {method})"),
                            },
                            Err(err) => {
                                log::warn!("Error handling request: {err}");
                                stream.shutdown(Shutdown::Both)?;
                                return Err(err);
                            }
                        }

                        while let Ok(msg) = rx.try_recv() {
                            Self::send(&mut stream, &msg)?;
                        }
                    }

                    Ok(())
                });
            }

            while let Ok(msg) = self.push_queue()?.pop_event() {
                log::trace!("Sending message from push queue: {msg}");

                self.bus_mut()?.broadcast(msg);
            }

            // Sleep for a bit to avoid a tight loop
            thread::sleep(time::Duration::from_millis(250));
        }

        Ok(())
    }

    #[cfg(feature = "jsonrpc")]
    fn receive(handle: &mut DeviceHandle, stream: &mut UnixStream) -> Result<Method> {
        handle.on_message(stream)
    }

    #[cfg(feature = "jsonrpc")]
    fn send(stream: &mut UnixStream, msg: &Event) -> Result<()> {
        log::debug!("Sending push event: {msg}");

        let push_req = smol_jsonrpc::Request::new()
            .with_method(msg.method().to_str())
            .with_params(msg);

        let mut json_str = serde_json::to_string(&push_req)?;
        json_str += "\n";

        stream.write_all(json_str.as_bytes())?;
        stream.flush()?;

        Ok(())
    }
}

#[cfg(feature = "jsonrpc")]
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(path) = self.socket_path.as_ref() {
            let _ = std::fs::remove_file(path);
        }
    }
}
