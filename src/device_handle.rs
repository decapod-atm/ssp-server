#![allow(dead_code)]

use std::io::{Read, Write};
#[cfg(feature = "jsonrpc")]
use std::os::unix::net::UnixStream;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering},
    mpsc, Arc,
};
use std::thread;
use std::time;

use parking_lot::{Mutex, MutexGuard};
use serialport::TTYPort;

#[cfg(feature = "jsonrpc")]
use smol_jsonrpc::{Error as RpcError, Request, Response};

#[cfg(feature = "jsonrpc")]
use ssp::jsonrpc::{jsonrpc_id, set_jsonrpc_id};
use ssp::{CommandOps, MessageOps, ResponseOps, Result};

use crate::{continue_on_err, encryption_key};

mod inner;

/// Timeout for waiting for lock on a mutex (milliseconds).
pub const LOCK_TIMEOUT_MS: u64 = 5_000;
/// Timeout for waiting for serial communication (milliseconds).
pub const SERIAL_TIMEOUT_MS: u64 = 10_000;
/// Minimum polling interval between messages (milliseconds).
pub const MIN_POLLING_MS: u64 = 200;
/// Medium polling interval between messages (milliseconds).
pub const MED_POLLING_MS: u64 = 650;
/// Maximum polling interval between messages (milliseconds).
#[allow(dead_code)]
pub const MAX_POLLING_MS: u64 = 1_000;
/// Timeout for retrieving an event from a queue (milliseconds)
pub const QUEUE_TIMEOUT_MS: u128 = 50;
/// Default serial connection BAUD rate (bps).
pub const BAUD_RATE: u32 = 9_600;

pub(crate) static SEQ_FLAG: AtomicBool = AtomicBool::new(false);
static POLLING_INIT: AtomicBool = AtomicBool::new(false);

static ESCROWED: AtomicBool = AtomicBool::new(false);
static ESCROWED_AMOUNT: AtomicU32 = AtomicU32::new(0);

static ENABLED: AtomicBool = AtomicBool::new(false);

static CASHBOX_ATTACHED: AtomicBool = AtomicBool::new(true);

// Time when a device reset was initiated.
static RESET_TIME: AtomicU64 = AtomicU64::new(0);
// Timeout for waiting for the device to reset (seconds).
const RESET_TIMEOUT_SECS: u64 = 60;

static PROTOCOL_VERSION: AtomicU8 = AtomicU8::new(6);

static INTERACTIVE: AtomicBool = AtomicBool::new(false);

pub(crate) fn sequence_flag() -> ssp::SequenceFlag {
    SEQ_FLAG.load(Ordering::Relaxed).into()
}

pub(crate) fn set_sequence_flag(flag: ssp::SequenceFlag) {
    SEQ_FLAG.store(flag.into(), Ordering::SeqCst);
}

// Whether the polling routine has started.
fn polling_inited() -> bool {
    POLLING_INIT.load(Ordering::Relaxed)
}

// Sets the flag indicating whether the polling routine started.
fn set_polling_inited(inited: bool) {
    POLLING_INIT.store(inited, Ordering::SeqCst);
}

pub(crate) fn escrowed() -> bool {
    ESCROWED.load(Ordering::Relaxed)
}

pub(crate) fn set_escrowed(escrowed: bool) -> bool {
    let last = ESCROWED.load(Ordering::Relaxed);
    ESCROWED.store(escrowed, Ordering::SeqCst);
    last
}

pub(crate) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub(crate) fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::SeqCst);
}

pub(crate) fn escrowed_amount() -> ssp::ChannelValue {
    ESCROWED_AMOUNT.load(Ordering::Relaxed).into()
}

pub(crate) fn set_escrowed_amount(amount: ssp::ChannelValue) -> ssp::ChannelValue {
    let last = ssp::ChannelValue::from(ESCROWED_AMOUNT.load(Ordering::Relaxed));
    ESCROWED_AMOUNT.store(amount.into(), Ordering::SeqCst);
    last
}

/// Gets whether the csahboxed is attached to the device.
pub fn cashbox_attached() -> bool {
    CASHBOX_ATTACHED.load(Ordering::Relaxed)
}

/// Sets whether the csahboxed is attached to the device.
pub fn set_cashbox_attached(attached: bool) -> bool {
    let last = cashbox_attached();
    CASHBOX_ATTACHED.store(attached, Ordering::SeqCst);
    last
}

pub(crate) fn resetting() -> bool {
    reset_time() != 0
}

pub(crate) fn reset_time() -> u64 {
    RESET_TIME.load(Ordering::Relaxed)
}

pub(crate) fn set_reset_time(time: u64) -> u64 {
    let last = reset_time();
    RESET_TIME.store(time, Ordering::SeqCst);
    last
}

pub(crate) fn protocol_version() -> ssp::ProtocolVersion {
    PROTOCOL_VERSION.load(Ordering::Relaxed).into()
}

pub(crate) fn set_protocol_version(protocol: ssp::ProtocolVersion) -> ssp::ProtocolVersion {
    let last = protocol_version();
    PROTOCOL_VERSION.store(protocol.into(), Ordering::SeqCst);
    last
}

pub(crate) fn interactive() -> bool {
    INTERACTIVE.load(Ordering::Relaxed)
}

pub(crate) fn set_interactive(val: bool) -> bool {
    let last = interactive();
    INTERACTIVE.store(val, Ordering::SeqCst);
    last
}

/// Polling interactivity mode.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PollMode {
    /// Automically handle polling events.
    Auto,
    /// Interactively handle polling events.
    Interactive,
}

/// Receiver end of the device-sent event queue.
///
/// Owner of the receiver can regularly attempt to pop events from the queue,
/// and decide how to handle any returned event(s).
///
/// Example:
///
/// ```rust, no_run
/// # use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
/// # fn main() -> ssp::Result<()> {
/// let stop_polling = Arc::new(AtomicBool::new(false));
/// let poll_mode = ssp_server::PollMode::Interactive;
///
/// let mut handle = ssp_server::DeviceHandle::new("/dev/ttyUSB0")?;
///
/// let rx: ssp_server::PushEventReceiver = handle.start_background_polling_with_queue(
///     Arc::clone(&stop_polling),
///     poll_mode,
/// )?;
///
/// // Pop events from the queue
/// loop {
///     while let Ok(event) = rx.pop_event() {
///         log::debug!("Received an event: {event}");
///         // do stuff in response to the event...
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub struct PushEventReceiver(pub mpsc::Receiver<ssp::Event>);

impl PushEventReceiver {
    /// Creates a new [PushEventReceiver] from the provided `queue`.
    pub fn new(queue: mpsc::Receiver<ssp::Event>) -> Self {
        Self(queue)
    }

    /// Attempt to pop an event from the queue.
    ///
    /// Returns `Err(_)` if an event could not be retrieved before the timeout.
    pub fn pop_event(&self) -> Result<ssp::Event> {
        let now = time::Instant::now();
        let queue = &self.0;

        while now.elapsed().as_millis() < QUEUE_TIMEOUT_MS {
            if let Ok(evt) = queue.try_recv() {
                return Ok(evt);
            }
        }

        Err(ssp::Error::QueueTimeout)
    }
}

/// Handle for communicating with a SSP-enabled device over serial.
///
/// ```no_run
/// let _handle = ssp_server::DeviceHandle::new("/dev/ttyUSB0").unwrap();
/// ```
pub struct DeviceHandle {
    serial_port: Arc<Mutex<TTYPort>>,
    generator: ssp::GeneratorKey,
    modulus: ssp::ModulusKey,
    random: ssp::RandomKey,
    fixed_key: ssp::FixedKey,
    key: Arc<Mutex<Option<ssp::AesKey>>>,
}

impl DeviceHandle {
    /// Creates a new [DeviceHandle] with a serial connection over the supplied serial device.
    pub fn new(serial_path: &str) -> Result<Self> {
        // For details on the following setup, see sections 5.4 & 7 in the SSP implementation guide
        let serial_port = Arc::new(Mutex::new(
            serialport::new(serial_path, BAUD_RATE)
                // disable flow control serial lines
                .flow_control(serialport::FlowControl::None)
                // eight-bit data size
                .data_bits(serialport::DataBits::Eight)
                // no control bit parity
                .parity(serialport::Parity::None)
                // two bit stop
                .stop_bits(serialport::StopBits::Two)
                // serial device times out after 10 seconds, so do we
                .timeout(time::Duration::from_millis(SERIAL_TIMEOUT_MS))
                // get back a TTY port for POSIX systems, Windows is not supported
                .open_native()?,
        ));

        let mut prime_gen = ssp::primes::Generator::from_entropy();

        let mut generator = ssp::GeneratorKey::from_generator(&mut prime_gen);
        let mut modulus = ssp::ModulusKey::from_generator(&mut prime_gen);

        // Modulus key must be smaller than the Generator key
        let mod_inner = modulus.as_inner();
        let gen_inner = generator.as_inner();

        if gen_inner > mod_inner {
            modulus = gen_inner.into();
            generator = mod_inner.into();
        }

        let random = ssp::RandomKey::from_entropy();
        let fixed_key = ssp::FixedKey::from_inner(ssp::DEFAULT_FIXED_KEY_U64);
        let key = Arc::new(Mutex::new(None));

        Ok(Self {
            serial_port,
            generator,
            modulus,
            random,
            fixed_key,
            key,
        })
    }

    /// Starts background polling routine to regularly send [PollCommand] messages to the device.
    ///
    /// **Args**
    ///
    /// - `stop_polling`: used to control when the polling routine should stop sending polling messages.
    ///
    /// If background polling has already started, the function just returns.
    ///
    /// Example:
    ///
    /// ```rust, no_run
    /// # use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    /// # fn main() -> ssp::Result<()> {
    /// let stop_polling = Arc::new(AtomicBool::new(false));
    ///
    /// let handle = ssp_server::DeviceHandle::new("/dev/ttyUSB0")?;
    ///
    /// handle.start_background_polling(Arc::clone(&stop_polling))?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn start_background_polling(&self, stop_polling: Arc<AtomicBool>) -> Result<()> {
        if polling_inited() {
            Err(ssp::Error::PollingReinit)
        } else {
            // Set the global flag to disallow multiple background polling threads.
            set_polling_inited(true);

            let serial_port = Arc::clone(&self.serial_port);
            let end_polling = Arc::clone(&stop_polling);
            let key = Arc::clone(&self.key);

            thread::spawn(move || -> Result<()> {
                let mut now = time::Instant::now();

                while !end_polling.load(Ordering::Relaxed) {
                    if now.elapsed().as_millis() > MED_POLLING_MS as u128 {
                        now = time::Instant::now();

                        if resetting() {
                            continue;
                        }

                        let mut locked_port = continue_on_err!(
                            Self::lock_serial_port(&serial_port),
                            "Failed to lock serial port in background polling routine"
                        );
                        let key = continue_on_err!(
                            Self::lock_encryption_key(&key),
                            "Failed to lock encryption key in background polling routine"
                        );

                        let mut message = ssp::PollCommand::new();

                        let res = if let Some(key) = key.as_ref() {
                            continue_on_err!(
                                Self::poll_encrypted_message(&mut locked_port, &mut message, key),
                                "Failed poll command in background polling routine"
                            )
                        } else {
                            continue_on_err!(
                                Self::poll_message_variant(&mut locked_port, &mut message),
                                "Failed poll command in background polling routine"
                            )
                        };

                        let status = res.as_response().response_status();

                        if status.is_ok() {
                            let poll_res = continue_on_err!(
                                res.into_poll_response(),
                                "Failed to convert poll response in background polling routine"
                            );
                            let last_statuses = poll_res.last_response_statuses();

                            log::debug!("Successful poll command, last statuses: {last_statuses}");
                        } else {
                            log::warn!("Failed poll command, response status: {status}");
                        }
                    }

                    thread::sleep(time::Duration::from_millis(MED_POLLING_MS / 3));
                }

                // Now that polling finished, reset the flag to allow another background routine to
                // start.
                set_polling_inited(false);

                Ok(())
            });

            Ok(())
        }
    }

    /// Starts background polling routine to regularly send [PollCommand] messages to the device,
    /// with an additional event queue for sending push events from the device to the host.
    ///
    /// **Args**
    ///
    /// - `stop_polling`: used to control when the polling routine should stop sending polling messages.
    ///
    /// If background polling has already started, the function just returns.
    ///
    /// Returns an event queue receiver that the caller can use to receive device-sent events.
    ///
    /// Example:
    ///
    /// ```rust, no_run
    /// # use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    /// # fn main() -> ssp::Result<()> {
    /// let stop_polling = Arc::new(AtomicBool::new(false));
    /// let poll_mode = ssp_server::PollMode::Interactive;
    ///
    /// let handle = ssp_server::DeviceHandle::new("/dev/ttyUSB0")?;
    ///
    /// let rx = handle.start_background_polling_with_queue(
    ///     Arc::clone(&stop_polling),
    ///     poll_mode,
    /// )?;
    ///
    /// // Pop events from the queue
    /// loop {
    ///     while let Ok(event) = rx.pop_event() {
    ///         log::debug!("Received an event: {event}");
    ///         // do stuff in response to the event...
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn start_background_polling_with_queue(
        &self,
        stop_polling: Arc<AtomicBool>,
        poll_mode: PollMode,
    ) -> Result<PushEventReceiver> {
        if polling_inited() {
            Err(ssp::Error::PollingReinit)
        } else {
            // Set the global flag to disallow multiple background polling threads.
            set_polling_inited(true);

            if poll_mode == PollMode::Interactive {
                set_interactive(true);
            }

            let serial_port = Arc::clone(&self.serial_port);
            let end_polling = Arc::clone(&stop_polling);
            let key = Arc::clone(&self.key);

            let (tx, rx) = mpsc::channel();

            thread::spawn(move || -> Result<()> {
                let mut now = time::Instant::now();

                while !end_polling.load(Ordering::Relaxed) {
                    if now.elapsed().as_millis() >= MIN_POLLING_MS as u128 {
                        now = time::Instant::now();

                        if resetting() {
                            continue;
                        }

                        let mut locked_port = continue_on_err!(
                            Self::lock_serial_port(&serial_port),
                            "Failed to lock serial port in background polling routine"
                        );

                        let key = continue_on_err!(
                            Self::lock_encryption_key(&key),
                            "Failed to lock encryption key in background polling routine"
                        );

                        if (escrowed() && poll_mode == PollMode::Interactive) || enabled() {
                            // Do not automatically poll when device has a bill in escrow,
                            // and the user is in interactive mode.
                            //
                            // Sending a poll with bill in escrow stacks the bill.
                            //
                            // Sit in a busy loop until the user sends a stack/reject command.

                            // Send hold command to keep note in escrow until `stack` or `reject`
                            // is sent.

                            let mut message = ssp::HoldCommand::new();

                            continue_on_err!(
                                Self::poll_message(&mut locked_port, &mut message, key.as_ref()),
                                "Failed hold command"
                            );

                            continue;
                        }

                        let mut message = ssp::PollCommand::new();

                        let res = continue_on_err!(
                            Self::poll_message(&mut locked_port, &mut message, key.as_ref()),
                            "Failed poll command"
                        );

                        let status = res.as_response().response_status();
                        if status.is_ok() {
                            let poll_res = continue_on_err!(
                                res.into_poll_response(),
                                "Failed to convert poll response in background polling routine"
                            );

                            Self::parse_events(&poll_res, &tx)?;
                        } else if status.to_u8() == 0 {
                            log::info!("Device returned a null response: {}", res.as_response());
                            log::trace!("Response data: {:x?}", res.as_response().buf());
                        } else {
                            log::warn!("Failed poll command, response status: {status}");
                        }
                    }

                    thread::sleep(time::Duration::from_millis(MIN_POLLING_MS / 3));
                }

                // Now that polling finished, reset the flag to allow another background routine to
                // start.
                set_polling_inited(false);

                Ok(())
            });

            Ok(PushEventReceiver::new(rx))
        }
    }

    fn poll_resetting(
        serial_port: &mut TTYPort,
        key: Option<&ssp::AesKey>,
        tx: Option<&mpsc::Sender<ssp::Event>>,
    ) -> Result<()> {
        use std::ops::Sub;

        let reset_time = time::Instant::now().sub(time::Duration::from_secs(reset_time()));
        let mut message = ssp::PollCommand::new();

        while (1..RESET_TIMEOUT_SECS).contains(&reset_time.elapsed().as_secs()) {
            let elapsed = reset_time.elapsed().as_secs();
            let res = continue_on_err!(
                Self::poll_message(serial_port, &mut message, key),
                format!("Device is still resetting, elapsed time: {elapsed}")
            );
            if res.as_response().response_status().is_ok() {
                set_reset_time(0);

                if let Some(tx) = tx {
                    continue_on_err!(
                        tx.send(ssp::Event::from(ssp::ResetEvent::new())),
                        "Failed to send Reset event"
                    );
                }

                break;
            }
        }

        Ok(())
    }

    #[cfg(feature = "jsonrpc")]
    pub fn on_message(&mut self, stream: &mut UnixStream) -> Result<ssp::Method> {
        stream.set_nonblocking(true)?;

        let mut message_buf = vec![0u8; 1024];
        let mut idx = 0;

        while let Ok(ret) = stream.read(&mut message_buf) {
            if ret == 0 {
                // Client hung up the socket, so let the caller know to shutdown the stream
                return Ok(ssp::Method::Shutdown);
            }

            if idx >= message_buf.len() {
                message_buf.resize(2 * message_buf.len(), 0u8);
            }

            idx += ret;

            if message_buf.contains(&b'\n') {
                break;
            }
        }

        let message_string = std::str::from_utf8(message_buf[..idx].as_ref()).unwrap_or("");
        if message_string.is_empty() || message_string.contains("ready") {
            return Ok(ssp::Method::Enable);
        }

        for message in message_string.split('\n') {
            if !message.is_empty() {
                log::debug!("Received message: {message}");

                let message = match serde_json::from_str::<Request>(message) {
                    Ok(msg) => msg,
                    Err(err) => {
                        log::warn!("Expected valid JSON-RPC request, error: {err}");
                        continue;
                    }
                };

                let event = ssp::Event::from(&message);
                let method = event.method();
                log::debug!("Message method: {method}");

                let jsonrpc_id = message.id().unwrap_or(jsonrpc_id());
                set_jsonrpc_id(jsonrpc_id);

                match method {
                    ssp::Method::Disable | ssp::Method::Stop => self.on_disable(stream, &event)?,
                    ssp::Method::Enable | ssp::Method::Accept => self.on_enable(stream, &event)?,
                    ssp::Method::Reject => self.on_reject(stream, &event)?,
                    ssp::Method::Stack => self.on_stack(stream, &event)?,
                    ssp::Method::StackerFull => self.on_stacker_full(stream, &event)?,
                    ssp::Method::Status => self.on_status(stream, &event)?,
                    ssp::Method::Reset => self.on_reset(stream, &event)?,
                    ssp::Method::Dispense => self.on_dispense(stream, &event)?,
                    _ => return Err(ssp::Error::JsonRpc("unsupported method".into())),
                }

                return Ok(method);
            }
        }

        Ok(ssp::Method::Disable)
    }

    /// Message handler for [Disable](ssp::Event::DisableEvent) events.
    ///
    /// Exposed to help with creating a custom message handler.
    #[cfg(feature = "jsonrpc")]
    pub fn on_disable(&self, stream: &mut UnixStream, _event: &ssp::Event) -> Result<()> {
        self.disable()?;

        let mut res = Response::from(ssp::Event::from(ssp::DisableEvent::new()));
        res.set_id(jsonrpc_id());

        let mut res_str = serde_json::to_string(&res)?;
        res_str += "\n";

        stream.write_all(res_str.as_bytes())?;

        Ok(())
    }

    /// Message handler for [Enable](ssp::Event::EnableEvent) events.
    ///
    /// Exposed to help with creating a custom message handler.
    #[cfg(feature = "jsonrpc")]
    pub fn on_enable(&self, stream: &mut UnixStream, event: &ssp::Event) -> Result<()> {
        // perform full init sequence,
        // only sending EnableCommand does not bring the device online...
        let enable_event = ssp::EnableEvent::try_from(event)?;
        self.enable_device(enable_event.protocol_version())?;

        let mut res = Response::from(ssp::Event::from(enable_event));
        res.set_id(jsonrpc_id());

        let mut res_str = serde_json::to_string(&res)?;
        res_str += "\n";

        stream.write_all(res_str.as_bytes())?;

        Ok(())
    }

    /// Message handler for [Reject](ssp::Event::RejectEvent) events.
    ///
    /// Exposed to help with creating a custom message handler.
    #[cfg(feature = "jsonrpc")]
    pub fn on_reject(&self, stream: &mut UnixStream, _event: &ssp::Event) -> Result<()> {
        self.reject()?;

        let mut res = Response::from(ssp::Event::from(ssp::RejectEvent::new()));
        res.set_id(jsonrpc_id());

        let mut res_str = serde_json::to_string(&res)?;
        res_str += "\n";

        stream.write_all(res_str.as_bytes())?;

        Ok(())
    }

    /// Message handler for [Stack](ssp::Event::StackEvent) events.
    ///
    /// Exposed to help with creating a custom message handler.
    #[cfg(feature = "jsonrpc")]
    pub fn on_stack(&self, stream: &mut UnixStream, _event: &ssp::Event) -> Result<()> {
        let value = self.stack()?;

        let mut res = Response::from(ssp::Event::from(ssp::StackEvent::from(value)));
        res.set_id(jsonrpc_id());

        let mut res_str = serde_json::to_string(&res)?;
        res_str += "\n";

        stream.write_all(res_str.as_bytes())?;

        Ok(())
    }

    /// Message handler for [StackerFull](ssp::Event::StackerFullEvent) events.
    ///
    /// Exposed to help with creating a custom message handler.
    #[cfg(feature = "jsonrpc")]
    pub fn on_stacker_full(&self, _stream: &mut UnixStream, _event: &ssp::Event) -> Result<()> {
        Err(ssp::Error::JsonRpc(
            "StackerFull handler unimplemented".into(),
        ))
    }

    /// Message handler for [Status](ssp::Event::StatusEvent) events.
    ///
    /// Exposed to help with creating a custom message handler.
    #[cfg(feature = "jsonrpc")]
    pub fn on_status(&self, stream: &mut UnixStream, _event: &ssp::Event) -> Result<()> {
        let (data, dataset_version) = {
            let mut serial_port = self.serial_port()?;
            let key = self.encryption_key()?;

            (
                self.unit_data_inner(&mut serial_port, key.as_ref())?,
                Self::dataset_version_inner(&mut serial_port, key.as_ref())?,
            )
        };

        let cashbox_attached = cashbox_attached();
        let status = ssp::DeviceStatus::from(data)
            .with_dataset_version(dataset_version.dataset_version()?)
            .with_cashbox_attached(cashbox_attached);

        let event = if cashbox_attached {
            ssp::StatusEvent::new(status)
        } else {
            ssp::StatusEvent::new(status.with_response_status(ssp::ResponseStatus::CashboxRemoved))
        };

        let mut res = Response::from(ssp::Event::from(event));
        res.set_id(jsonrpc_id());

        let res_str = serde_json::to_string(&res)? + "\n";

        stream.write_all(res_str.as_bytes())?;

        Ok(())
    }

    /// Message handler for [Status](ssp::Event::StatusEvent) events.
    ///
    /// Exposed to help with creating a custom message handler.
    #[cfg(feature = "jsonrpc")]
    pub fn on_reset(&self, stream: &mut UnixStream, _event: &ssp::Event) -> Result<()> {
        match self.full_reset() {
            Ok(_) => {
                let res =
                    Response::from(ssp::Event::from(ssp::ResetEvent::new())).with_id(jsonrpc_id());

                let res_str = serde_json::to_string(&res)? + "\n";

                log::debug!("Successfully reset device: {res_str}");

                stream.write_all(res_str.as_bytes())?;

                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    /// Performs the full reset protocol to restart a device.
    pub fn full_reset(&self) -> Result<()> {
        use serialport::SerialPort;

        self.reset()?;

        let now = time::Instant::now();

        // Wait a bit for the device to reset, and re-open the port.
        thread::sleep(time::Duration::from_secs(20));

        let mut serial_port = self.serial_port()?;
        // Clear the serial port to simulate closing and opening the port
        serial_port.clear(serialport::ClearBuffer::All)?;

        while now.elapsed().as_secs() < RESET_TIMEOUT_SECS {
            if let Ok(res) = self.sync_inner(&mut serial_port) {
                if res.response_status().is_ok() {
                    set_reset_time(0);

                    if let Err(err) = self.enable_device_inner(&mut serial_port, protocol_version())
                    {
                        log::error!("Error enabling device after reset: {err}");
                    }

                    if interactive() {
                        // if the server is running in interactive mode, disable until the client
                        // re-enables the device.
                        if let Err(err) = self.disable_inner(&mut serial_port, None) {
                            log::error!("Error disabling device after reset: {err}");
                        }
                    }

                    let poll_res = self.poll_inner(&mut serial_port, None)?;
                    if poll_res.response_status().is_ok() {
                        log::debug!("Successfully reset device");

                        return Ok(());
                    } else {
                        return Err(ssp::Error::InvalidStatus((
                            poll_res.response_status(),
                            ssp::ResponseStatus::Ok,
                        )));
                    }
                }
            }
        }

        Err(ssp::Error::JsonRpc("failed to reset device".into()))
    }

    /// Message handle for dispense request using a
    /// [PayoutDenominationList](ssp::PayoutDenominationList).
    ///
    /// User is responsible for parsing a request into a valid list.
    ///
    /// Exposed to help with creating a custom message handler.
    #[cfg(feature = "jsonrpc")]
    pub fn on_dispense(&self, stream: &mut UnixStream, event: &ssp::Event) -> Result<()> {
        let inner_event = event.payload().as_dispense_event()?;

        let res = if let Err(err) = self.payout_by_denomination(inner_event.as_inner()) {
            Response::new()
                .with_id(jsonrpc_id())
                .with_error(RpcError::new().with_message(format!("{err}").as_str()))
        } else {
            Response::new().with_id(jsonrpc_id())
        };

        let res_str = serde_json::to_string(&res)? + "\n";

        stream.write_all(res_str.as_bytes())?;

        Ok(())
    }

    /// Acquires a lock on the serial port used for communication with the acceptor device.
    pub fn serial_port(&self) -> Result<MutexGuard<'_, TTYPort>> {
        Self::lock_serial_port(&self.serial_port)
    }

    pub(crate) fn lock_serial_port(
        serial_port: &Arc<Mutex<TTYPort>>,
    ) -> Result<MutexGuard<'_, TTYPort>> {
        serial_port
            .try_lock_for(time::Duration::from_millis(SERIAL_TIMEOUT_MS))
            .ok_or(ssp::Error::SerialPort(
                "timed out locking serial port".into(),
            ))
    }

    /// Acquires a lock on the AES encryption key.
    pub fn encryption_key(&self) -> Result<MutexGuard<'_, Option<ssp::AesKey>>> {
        Self::lock_encryption_key(&self.key)
    }

    pub(crate) fn lock_encryption_key(
        key: &Arc<Mutex<Option<ssp::AesKey>>>,
    ) -> Result<MutexGuard<'_, Option<ssp::AesKey>>> {
        key.try_lock_for(time::Duration::from_millis(LOCK_TIMEOUT_MS))
            .ok_or(ssp::Error::Io("timed out locking encryption key".into()))
    }

    /// Creates a new [GeneratorKey](ssp::GeneratorKey) from system entropy.
    pub fn new_generator_key(&mut self) {
        self.generator = ssp::GeneratorKey::from_entropy();
        self.reset_key();
    }

    /// Creates a new [ModulusKey](ssp::ModulusKey) from system entropy.
    pub fn new_modulus_key(&mut self) {
        let mut modulus = ssp::ModulusKey::from_entropy();

        // Modulus key must be smaller than the Generator key
        let gen_inner = self.generator.as_inner();
        let mod_inner = modulus.as_inner();

        if gen_inner < mod_inner {
            self.generator = mod_inner.into();
            modulus = gen_inner.into();
        }

        self.modulus = modulus;

        self.reset_key();
    }

    /// Creates a new [RandomKey](ssp::RandomKey) from system entropy.
    pub fn new_random_key(&mut self) {
        self.random = ssp::RandomKey::from_entropy();
        self.reset_key();
    }

    fn generator_key(&self) -> &ssp::GeneratorKey {
        &self.generator
    }

    fn modulus_key(&self) -> &ssp::ModulusKey {
        &self.modulus
    }

    fn random_key(&self) -> &ssp::RandomKey {
        &self.random
    }

    fn set_key(&mut self, inter_key: ssp::IntermediateKey) -> Result<()> {
        let mut key = self.encryption_key()?;

        let mut new_key = ssp::AesKey::from(&self.fixed_key);
        let enc_key =
            ssp::EncryptionKey::from_keys(&inter_key, self.random_key(), self.modulus_key());

        new_key[8..].copy_from_slice(enc_key.as_inner().to_le_bytes().as_ref());

        key.replace(new_key);

        Ok(())
    }

    // Resets the Encryption key to none, requires a new key negotiation before performing eSSP
    // operations.
    fn reset_key(&mut self) -> Option<ssp::AesKey> {
        if let Ok(mut key) = self.encryption_key() {
            key.take()
        } else {
            None
        }
    }

    /// Sends a command to stack a bill in escrow.
    pub fn stack(&self) -> Result<ssp::ChannelValue> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::PollCommand::new();
        let res = Self::poll_message(&mut serial_port, &mut message, encryption_key!(self))?;

        let status = res.as_response().response_status();
        if status.is_ok() {
            set_escrowed(false);
            Ok(set_escrowed_amount(ssp::ChannelValue::default()))
        } else {
            Err(ssp::Error::InvalidStatus((status, ssp::ResponseStatus::Ok)))
        }
    }

    /// Send a [SetInhibitsCommand](ssp::SetInhibitsCommand) message to the device.
    ///
    /// No response is returned.
    ///
    /// The caller should wait a reasonable amount of time for the device
    /// to come back online before sending additional messages.
    pub fn set_inhibits(
        &self,
        enable_list: ssp::EnableBitfieldList,
    ) -> Result<ssp::SetInhibitsResponse> {
        let mut serial_port = self.serial_port()?;
        self.set_inhibits_inner(&mut serial_port, enable_list, encryption_key!(self))
    }

    fn set_inhibits_inner(
        &self,
        serial_port: &mut TTYPort,
        enable_list: ssp::EnableBitfieldList,
        key: Option<&ssp::AesKey>,
    ) -> Result<ssp::SetInhibitsResponse> {
        let mut message = ssp::SetInhibitsCommand::new();
        message.set_inhibits(enable_list)?;

        let res = Self::poll_message(serial_port, &mut message, key)?;

        res.into_set_inhibits_response()
    }

    /// Send a [ResetCommand](ssp::ResetCommand) message to the device.
    ///
    /// No response is returned.
    ///
    /// The caller should wait a reasonable amount of time for the device
    /// to come back online before sending additional messages.
    pub fn reset(&self) -> Result<()> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::ResetCommand::new();

        Self::set_message_sequence_flag(&mut message);

        serial_port.write_all(message.as_bytes())?;

        set_reset_time(
            time::SystemTime::now()
                .duration_since(time::UNIX_EPOCH)?
                .as_secs(),
        );

        Ok(())
    }

    /// Send a [PollCommand](ssp::PollCommand) message to the device.
    pub fn poll(&self) -> Result<ssp::PollResponse> {
        let mut serial_port = self.serial_port()?;

        self.poll_inner(&mut serial_port, encryption_key!(self))
    }

    fn poll_inner(
        &self,
        serial_port: &mut TTYPort,
        key: Option<&ssp::AesKey>,
    ) -> Result<ssp::PollResponse> {
        let mut message = ssp::PollCommand::new();

        Self::set_message_sequence_flag(&mut message);

        let response = Self::poll_message(serial_port, &mut message, key)?;

        response.into_poll_response()
    }

    /// Send a [PollWithAckCommand](ssp::PollWithAckCommand) message to the device.
    pub fn poll_with_ack(&self) -> Result<ssp::PollWithAckResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::PollWithAckCommand::new();

        Self::set_message_sequence_flag(&mut message);

        let response = Self::poll_message(&mut serial_port, &mut message, encryption_key!(self))?;

        response.into_poll_with_ack_response()
    }

    /// Send a [EventAckCommand](ssp::EventAckCommand) message to the device.
    pub fn event_ack(&self) -> Result<ssp::EventAckResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::EventAckCommand::new();

        Self::set_message_sequence_flag(&mut message);

        let response = Self::poll_message(&mut serial_port, &mut message, encryption_key!(self))?;

        response.into_event_ack_response()
    }

    /// Send a [RejectCommand](ssp::RejectCommand) message to the device.
    pub fn reject(&self) -> Result<ssp::RejectResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::RejectCommand::new();

        Self::set_message_sequence_flag(&mut message);

        let response = Self::poll_message(&mut serial_port, &mut message, encryption_key!(self))?;

        let res = response.into_reject_response()?;

        let status = res.response_status();

        if status.is_ok() {
            set_escrowed(false);
            set_escrowed_amount(ssp::ChannelValue::default());

            Ok(res)
        } else {
            Err(ssp::Error::InvalidStatus((status, ssp::ResponseStatus::Ok)))
        }
    }

    /// Send a [SyncCommand](ssp::SyncCommand) message to the device.
    pub fn sync(&self) -> Result<ssp::SyncResponse> {
        let mut serial_port = self.serial_port()?;

        self.sync_inner(&mut serial_port)
    }

    fn sync_inner(&self, serial_port: &mut TTYPort) -> Result<ssp::SyncResponse> {
        let mut message = ssp::SyncCommand::new();

        let response = Self::poll_message(serial_port, &mut message, encryption_key!(self))?;

        set_sequence_flag(ssp::SequenceFlag::from(0));

        response.into_sync_response()
    }

    /// Starts the device by performing the full initialization sequence.
    pub fn enable_device(
        &self,
        protocol_version: ssp::ProtocolVersion,
    ) -> Result<ssp::EnableResponse> {
        let mut serial_port = self.serial_port()?;

        self.enable_device_inner(&mut serial_port, protocol_version)
    }

    fn enable_device_inner(
        &self,
        serial_port: &mut TTYPort,
        protocol_version: ssp::ProtocolVersion,
    ) -> Result<ssp::EnableResponse> {
        self.poll_inner(serial_port, None)?;

        // FIXME: make these *_inner functions accept a locked encryption key also to avoid
        // deadlocks from locking and unlocking the mutex
        let status_res = self.unit_data_inner(serial_port, None)?;
        self.serial_number_inner(serial_port, None)?;

        if status_res.protocol_version() != protocol_version {
            self.host_protocol_version_inner(serial_port, protocol_version, None)?;
            set_protocol_version(protocol_version);
        }

        let enable_list = ssp::EnableBitfieldList::from([
            ssp::EnableBitfield::from(0xff),
            ssp::EnableBitfield::from(0xff),
            #[cfg(feature = "nv200")]
            ssp::EnableBitfield::from(0xff),
        ]);

        self.set_inhibits_inner(serial_port, enable_list, None)?;
        self.channel_value_data_inner(serial_port, None)?;

        self.enable_inner(serial_port, None)
    }

    /// Send a [EnableCommand](ssp::EnableCommand) message to the device.
    pub fn enable(&self) -> Result<ssp::EnableResponse> {
        let mut serial_port = self.serial_port()?;
        self.enable_inner(&mut serial_port, encryption_key!(self))
    }

    fn enable_inner(
        &self,
        serial_port: &mut TTYPort,
        key: Option<&ssp::AesKey>,
    ) -> Result<ssp::EnableResponse> {
        let mut message = ssp::EnableCommand::new();

        let response = Self::poll_message(serial_port, &mut message, key)?;

        set_enabled(true);

        response.into_enable_response()
    }

    /// Send a [DisableCommand](ssp::DisableCommand) message to the device.
    pub fn disable(&self) -> Result<ssp::DisableResponse> {
        let mut serial_port = self.serial_port()?;
        self.disable_inner(&mut serial_port, encryption_key!(self))
    }

    fn disable_inner(
        &self,
        serial_port: &mut TTYPort,
        key: Option<&ssp::AesKey>,
    ) -> Result<ssp::DisableResponse> {
        let mut message = ssp::DisableCommand::new();

        let response = Self::poll_message(serial_port, &mut message, key)?;

        response.into_disable_response()
    }

    /// Send a [DisplayOffCommand](ssp::DisplayOffCommand) message to the device.
    pub fn display_off(&self) -> Result<ssp::DisplayOffResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::DisplayOffCommand::new();

        let response = Self::poll_message(&mut serial_port, &mut message, encryption_key!(self))?;

        response.into_display_off_response()
    }

    /// Send a [DisplayOnCommand](ssp::DisplayOnCommand) message to the device.
    pub fn display_on(&self) -> Result<ssp::DisplayOnResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::DisplayOnCommand::new();

        let response = Self::poll_message(&mut serial_port, &mut message, encryption_key!(self))?;

        response.into_display_on_response()
    }

    /// Send an [EmptyCommand](ssp::EmptyCommand) message to the device.
    pub fn empty(&self) -> Result<ssp::EmptyResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::EmptyCommand::new();

        if let Some(key) = (*self.encryption_key()?).as_ref() {
            let res = Self::poll_encrypted_message(&mut serial_port, &mut message, key)?;

            res.into_empty_response()
        } else {
            Err(ssp::Error::Encryption(ssp::ResponseStatus::KeyNotSet))
        }
    }

    /// Send an [SmartEmptyCommand](ssp::SmartEmptyCommand) message to the device.
    pub fn smart_empty(&self) -> Result<ssp::SmartEmptyResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::SmartEmptyCommand::new();

        if let Some(key) = self.encryption_key()?.as_ref() {
            let res = Self::poll_encrypted_message(&mut serial_port, &mut message, key)?;

            res.into_smart_empty_response()
        } else {
            Err(ssp::Error::Encryption(ssp::ResponseStatus::KeyNotSet))
        }
    }

    /// Send an [HostProtocolVersionCommand](ssp::HostProtocolVersionCommand) message to the device.
    pub fn host_protocol_version(
        &self,
        protocol_version: ssp::ProtocolVersion,
    ) -> Result<ssp::HostProtocolVersionResponse> {
        let mut serial_port = self.serial_port()?;

        self.host_protocol_version_inner(&mut serial_port, protocol_version, encryption_key!(self))
    }

    fn host_protocol_version_inner(
        &self,
        serial_port: &mut TTYPort,
        protocol_version: ssp::ProtocolVersion,
        key: Option<&ssp::AesKey>,
    ) -> Result<ssp::HostProtocolVersionResponse> {
        let mut message = ssp::HostProtocolVersionCommand::new();
        message.set_version(protocol_version);

        let response = Self::poll_message(serial_port, &mut message, key)?;

        response.into_host_protocol_version_response()
    }

    /// Send a [SerialNumberCommand](ssp::SerialNumberCommand) message to the device.
    pub fn serial_number(&self) -> Result<ssp::SerialNumberResponse> {
        let mut serial_port = self.serial_port()?;
        self.serial_number_inner(&mut serial_port, encryption_key!(self))
    }

    fn serial_number_inner(
        &self,
        serial_port: &mut TTYPort,
        key: Option<&ssp::AesKey>,
    ) -> Result<ssp::SerialNumberResponse> {
        let mut message = ssp::SerialNumberCommand::new();

        let res = Self::poll_message(serial_port, &mut message, key)?;

        res.into_serial_number_response()
    }

    /// Send a [SetGeneratorCommand](ssp::SetGeneratorCommand) message to the device.
    ///
    /// If the response is an `Err(_)`, or the response status is not
    /// [RsponseStatus::Ok](ssp::ResponseStatus::Ok), the caller should call
    /// [new_generator_key](Self::new_generator_key), and try again.
    pub fn set_generator(&self) -> Result<ssp::SetGeneratorResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::SetGeneratorCommand::new();
        message.set_generator(self.generator_key());

        let response = Self::poll_message(&mut serial_port, &mut message, None)?;

        response.into_set_generator_response()
    }

    /// Send a [SetModulusCommand](ssp::SetModulusCommand) message to the device.
    ///
    /// If the response is an `Err(_)`, or the response status is not
    /// [RsponseStatus::Ok](ssp::ResponseStatus::Ok), the caller should call
    /// [new_modulus_key](Self::new_modulus_key), and try again.
    pub fn set_modulus(&mut self) -> Result<ssp::SetModulusResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::SetModulusCommand::new();
        message.set_modulus(self.modulus_key());

        let response = Self::poll_message(&mut serial_port, &mut message, None)?;

        response.into_set_modulus_response()
    }

    /// Send a [RequestKeyExchangeCommand](ssp::RequestKeyExchangeCommand) message to the device.
    ///
    /// If the response is an `Err(_)`, or the response status is not
    /// [RsponseStatus::Ok](ssp::ResponseStatus::Ok), the caller should call
    /// [new_random_key](Self::new_random_key), and try again.
    pub fn request_key_exchange(&mut self) -> Result<ssp::RequestKeyExchangeResponse> {
        let res = {
            let mut serial_port = self.serial_port()?;

            let mut message = ssp::RequestKeyExchangeCommand::new();

            let inter_key = ssp::IntermediateKey::from_keys(
                self.generator_key(),
                self.random_key(),
                self.modulus_key(),
            );
            message.set_intermediate_key(&inter_key);

            let response = Self::poll_message(&mut serial_port, &mut message, None)?;

            response.into_request_key_exchange_response()?
        };

        // If the exchange was successful, set the new encryption key.
        if res.response_status().is_ok() {
            self.set_key(res.intermediate_key())?;
        }

        Ok(res)
    }

    /// Send a [SetEncryptionKeyCommand](ssp::SetEncryptionKeyCommand) message to the device.
    ///
    /// If the response is an `Err(_)`, or the response status is not
    /// [RsponseStatus::Ok](ssp::ResponseStatus::Ok), the caller should call
    /// [new_modulus_key](Self::new_modulus_key), and try again.
    pub fn set_encryption_key(&mut self) -> Result<ssp::SetEncryptionKeyResponse> {
        let mut message = ssp::SetEncryptionKeyCommand::new();

        let fixed_key = ssp::FixedKey::from_entropy();
        message.set_fixed_key(&fixed_key);

        let res = if let Some(key) = encryption_key!(self) {
            let mut serial_port = self.serial_port()?;
            Self::poll_encrypted_message(&mut serial_port, &mut message, key)
        } else {
            Err(ssp::Error::Encryption(ssp::ResponseStatus::KeyNotSet))
        };

        match res {
            Ok(m) => {
                self.fixed_key = fixed_key;
                m.into_set_encryption_key_response()
            }
            Err(err) => Err(err),
        }
    }

    /// Send a [EncryptionResetCommand](ssp::EncryptionResetCommand) message to the device.
    pub fn encryption_reset(&mut self) -> Result<ssp::EncryptionResetResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::EncryptionResetCommand::new();

        let response = Self::poll_message(&mut serial_port, &mut message, None)?;

        if response.as_response().response_status() == ssp::ResponseStatus::CommandCannotBeProcessed
        {
            Err(ssp::Error::Encryption(
                ssp::ResponseStatus::CommandCannotBeProcessed,
            ))
        } else {
            response.into_encryption_reset_response()
        }
    }

    /// Send a [SetupRequestCommand](ssp::SetupRequestCommand) message to the device.
    pub fn setup_request(&self) -> Result<ssp::SetupRequestResponse> {
        let mut serial_port = self.serial_port()?;
        self.setup_request_inner(&mut serial_port)
    }

    fn setup_request_inner(&self, serial_port: &mut TTYPort) -> Result<ssp::SetupRequestResponse> {
        let mut message = ssp::SetupRequestCommand::new();

        let response = Self::poll_message(serial_port, &mut message, encryption_key!(self))?;

        let res = response.into_setup_request_response()?;

        // configure global channel values
        let chan_vals = match res.protocol_version()? as u8 {
            0..=5 | 0xff => res.channel_values()?,
            _ => res.channel_values_long()?,
        };

        ssp::configure_channels(chan_vals.as_ref())?;

        Ok(res)
    }

    /// Send a [UnitDataCommand](ssp::UnitDataCommand) message to the device.
    pub fn unit_data(&self) -> Result<ssp::UnitDataResponse> {
        let mut serial_port = self.serial_port()?;

        self.unit_data_inner(&mut serial_port, encryption_key!(self))
    }

    fn unit_data_inner(
        &self,
        serial_port: &mut TTYPort,
        key: Option<&ssp::AesKey>,
    ) -> Result<ssp::UnitDataResponse> {
        let mut message = ssp::UnitDataCommand::new();

        let response = Self::poll_message(serial_port, &mut message, key)?;

        response.into_unit_data_response()
    }

    pub fn dataset_version(&self) -> Result<ssp::DatasetVersionResponse> {
        let mut serial_port = self.serial_port()?;

        Self::dataset_version_inner(&mut serial_port, encryption_key!(self))
    }

    pub fn dataset_version_inner(
        serial_port: &mut TTYPort,
        key: Option<&ssp::AesKey>,
    ) -> Result<ssp::DatasetVersionResponse> {
        let mut message = ssp::DatasetVersionCommand::new();

        let response = Self::poll_message(serial_port, &mut message, key)?;

        response.into_dataset_version_response()
    }

    /// Send a [ChannelValueDataCommand](ssp::ChannelValueDataCommand) message to the device.
    pub fn channel_value_data(&self) -> Result<ssp::ChannelValueDataResponse> {
        let mut serial_port = self.serial_port()?;

        self.channel_value_data_inner(&mut serial_port, encryption_key!(self))
    }

    fn channel_value_data_inner(
        &self,
        serial_port: &mut TTYPort,
        key: Option<&ssp::AesKey>,
    ) -> Result<ssp::ChannelValueDataResponse> {
        let mut message = ssp::ChannelValueDataCommand::new();

        let response = Self::poll_message(serial_port, &mut message, key)?;

        let res = response.into_channel_value_data_response()?;

        ssp::configure_channels(res.channel_values()?.as_ref())?;

        Ok(res)
    }

    /// Send a [LastRejectCodeCommand](ssp::LastRejectCodeCommand) message to the device.
    pub fn last_reject_code(&self) -> Result<ssp::LastRejectCodeResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::LastRejectCodeCommand::new();

        let response = Self::poll_message(&mut serial_port, &mut message, encryption_key!(self))?;

        response.into_last_reject_code_response()
    }

    /// Send a [HoldCommand](ssp::HoldCommand) message to the device.
    pub fn hold(&self) -> Result<ssp::HoldResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::HoldCommand::new();

        let response = Self::poll_message(&mut serial_port, &mut message, encryption_key!(self))?;

        response.into_hold_response()
    }

    /// Send a [GetBarcodeReaderConfigurationCommand](ssp::GetBarcodeReaderConfigurationCommand) message to the device.
    pub fn get_barcode_reader_configuration(
        &self,
    ) -> Result<ssp::GetBarcodeReaderConfigurationResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::GetBarcodeReaderConfigurationCommand::new();

        let response = Self::poll_message(&mut serial_port, &mut message, encryption_key!(self))?;

        response.into_get_barcode_reader_configuration_response()
    }

    /// Gets whether the device has barcode readers present.
    pub fn has_barcode_reader(&self) -> Result<bool> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::GetBarcodeReaderConfigurationCommand::new();

        let response = Self::poll_message(&mut serial_port, &mut message, encryption_key!(self))?;

        Ok(response
            .as_get_barcode_reader_configuration_response()?
            .hardware_status()
            != ssp::BarcodeHardwareStatus::None)
    }

    /// Send a [SetBarcodeReaderConfigurationCommand](ssp::SetBarcodeReaderConfigurationCommand) message to the device.
    pub fn set_barcode_reader_configuration(
        &self,
        config: ssp::BarcodeConfiguration,
    ) -> Result<ssp::SetBarcodeReaderConfigurationResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::SetBarcodeReaderConfigurationCommand::new();
        message.set_configuration(config);

        let response = Self::poll_message(&mut serial_port, &mut message, encryption_key!(self))?;

        response.into_set_barcode_reader_configuration_response()
    }

    /// Send a [GetBarcodeInhibitCommand](ssp::GetBarcodeInhibitCommand) message to the device.
    pub fn get_barcode_inhibit(&self) -> Result<ssp::GetBarcodeInhibitResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::GetBarcodeInhibitCommand::new();

        let response = Self::poll_message(&mut serial_port, &mut message, encryption_key!(self))?;

        response.into_get_barcode_inhibit_response()
    }

    /// Send a [SetBarcodeInhibitCommand](ssp::SetBarcodeInhibitCommand) message to the device.
    pub fn set_barcode_inhibit(
        &self,
        inhibit: ssp::BarcodeCurrencyInhibit,
    ) -> Result<ssp::SetBarcodeInhibitResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::SetBarcodeInhibitCommand::new();
        message.set_inhibit(inhibit);

        let response = Self::poll_message(&mut serial_port, &mut message, encryption_key!(self))?;

        response.into_set_barcode_inhibit_response()
    }

    /// Send a [GetBarcodeDataCommand](ssp::GetBarcodeDataCommand) message to the device.
    pub fn get_barcode_data(&self) -> Result<ssp::GetBarcodeDataResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::GetBarcodeDataCommand::new();

        let response = Self::poll_message(&mut serial_port, &mut message, encryption_key!(self))?;

        response.into_get_barcode_data_response()
    }

    /// Send a [ConfigureBezelCommand](ssp::ConfigureBezelCommand) message to the device.
    pub fn configure_bezel(
        &self,
        rgb: ssp::RGB,
        storage: ssp::BezelConfigStorage,
    ) -> Result<ssp::ConfigureBezelResponse> {
        let mut serial_port = self.serial_port()?;

        let mut message = ssp::ConfigureBezelCommand::new();
        message.set_rgb(rgb);
        message.set_config_storage(storage);

        let response = Self::poll_message(&mut serial_port, &mut message, encryption_key!(self))?;

        response.into_configure_bezel_response()
    }

    /// Dispenses notes from the device by sending a [PayoutByDenominationCommand] message.
    ///
    /// **NOTE**: this command requires encryption mode.
    ///
    /// Parameters:
    ///
    /// - `list`: list of [`PayoutDenomination`] requests
    ///
    /// Returns:
    ///
    /// - `Ok(())`
    /// - Err([`Error`](ssp::Error)) if an error occured
    pub fn payout_by_denomination(&self, list: &ssp::PayoutDenominationList) -> Result<()> {
        let mut serial_port = self.serial_port()?;
        let mut message = ssp::PayoutByDenominationCommand::new().with_payout_denominations(list);

        Self::payout_by_denomination_inner(&mut serial_port, &mut message, encryption_key!(self))
    }

    pub(crate) fn payout_by_denomination_inner(
        serial_port: &mut TTYPort,
        message: &mut ssp::PayoutByDenominationCommand,
        key: Option<&ssp::AesKey>,
    ) -> Result<()> {
        let response = Self::poll_message(serial_port, message, key)?;

        match response.as_response().response_status() {
            ssp::ResponseStatus::Ok => Ok(()),
            status => Err(ssp::Error::Status(status)),
        }
    }

    fn set_message_sequence_flag(message: &mut dyn CommandOps) {
        let mut sequence_id = message.sequence_id();
        sequence_id.set_flag(sequence_flag());
        message.set_sequence_id(sequence_id);
    }

    fn poll_message_variant(
        serial_port: &mut TTYPort,
        message: &mut dyn CommandOps,
    ) -> Result<ssp::MessageVariant> {
        use ssp::message::index;

        Self::set_message_sequence_flag(message);

        log::trace!(
            "Message type: {}, SEQID: {}",
            message.message_type(),
            message.sequence_id()
        );

        let mut attempt = 0;
        while let Err(_err) = serial_port.write_all(message.as_bytes()) {
            attempt += 1;
            log::warn!("Failed to send message, attempt #{attempt}");

            thread::sleep(time::Duration::from_millis(MIN_POLLING_MS));

            message.toggle_sequence_id();
        }

        // Set the global sequence flag to the opposite value for the next message
        set_sequence_flag(!message.sequence_id().flag());

        let mut buf = [0u8; ssp::len::MAX_MESSAGE];

        serial_port.read_exact(buf[..index::SEQ_ID].as_mut())?;

        let stx = buf[index::STX];
        if stx != ssp::STX {
            return Err(ssp::Error::InvalidSTX(stx));
        }

        serial_port.read_exact(buf[index::SEQ_ID..=index::LEN].as_mut())?;

        let buf_len = buf[index::LEN] as usize;
        let remaining = index::DATA + buf_len + 2; // data + CRC-16 bytes
        let total = buf_len + ssp::len::METADATA;

        serial_port.read_exact(buf[index::DATA..remaining].as_mut())?;

        ssp::MessageVariant::from_buf(buf[..total].as_ref(), message.message_type())
    }

    fn poll_encrypted_message(
        serial_port: &mut TTYPort,
        message: &mut dyn CommandOps,
        key: &ssp::AesKey,
    ) -> Result<ssp::MessageVariant> {
        let mut enc_cmd = ssp::EncryptedCommand::new();
        enc_cmd.set_message_data(message)?;

        let mut wrapped = enc_cmd.encrypt(key);

        wrapped.calculate_checksum();

        log::trace!("Encrypted message: {wrapped}");
        log::trace!("Encrypted data: {:x?}", wrapped.data());

        let response = Self::poll_message_variant(serial_port, &mut wrapped)?;

        if response.as_response().response_status() == ssp::ResponseStatus::KeyNotSet {
            return Err(ssp::Error::Encryption(ssp::ResponseStatus::KeyNotSet));
        }

        let wrapped_res = response.into_wrapped_encrypted_message()?;
        log::trace!("Encrypted response: {:x?}", wrapped_res.buf());

        // received an encrypted response, decrypt and process
        let dec_res = ssp::EncryptedResponse::decrypt(key, wrapped_res);
        log::trace!("Decrypted response: {dec_res}");
        log::trace!("Decrypted data: {:x?}", dec_res.data());

        let mut res = ssp::MessageVariant::new(message.command());
        res.as_response_mut().set_data(dec_res.data())?;
        res.as_response_mut().calculate_checksum();

        Ok(res)
    }

    fn poll_message(
        serial_port: &mut TTYPort,
        message: &mut dyn CommandOps,
        key: Option<&ssp::AesKey>,
    ) -> Result<ssp::MessageVariant> {
        if let Some(key) = key {
            Self::poll_encrypted_message(serial_port, message, key)
        } else {
            Self::poll_message_variant(serial_port, message)
        }
    }
}
