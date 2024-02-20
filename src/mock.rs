use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time;

use serialport::{SerialPort, TTYPort};

use ssp::{ResponseOps, Result};

use super::device_handle::{BAUD_RATE, SERIAL_TIMEOUT_MS};

/// Represents a mock SSP device used for integration tests.
///
/// The device can be configured to respond with a default message, e.g. for polling.
///
/// Messages generated by the device are sent encoded as SSP formatted message over a serial
/// connection.
pub struct MockDevice {
    serial_path: String,
    serial_port: TTYPort,
    msg_type: ssp::MessageType,
    response_status: ssp::ResponseStatus,
}

impl MockDevice {
    /// Creates a new [MockDevice].
    ///
    /// # Parameters
    ///
    /// - `path`: file path to the serial device, e.g. `/dev/ttyUSB0`
    /// - `msg_type`: SSP [MessageType](ssp::MessageType) for default replies
    /// - `response_status`: default SSP [ResponseStatus](ssp::ResponseStatus) for default replies
    pub fn new(
        path: &str,
        msg_type: ssp::MessageType,
        response_status: ssp::ResponseStatus,
    ) -> Result<Self> {
        let serial_port = serialport::new(path, BAUD_RATE)
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
            .open_native()?;

        Ok(Self {
            serial_path: path.into(),
            serial_port,
            msg_type,
            response_status,
        })
    }

    /// Gets a reference to the file path to the serial device.
    pub fn serial_path(&self) -> &str {
        self.serial_path.as_str()
    }

    /// Gets the default [MessageType](ssp::MessageType).
    pub const fn msg_type(&self) -> ssp::MessageType {
        self.msg_type
    }

    /// Sets the default [MessageType](ssp::MessageType).
    pub fn set_msg_type(&mut self, msg_type: ssp::MessageType) {
        self.msg_type = msg_type;
    }

    /// Gets the default [ResponseStatus](ssp::ResponseStatus).
    pub const fn response_status(&self) -> ssp::ResponseStatus {
        self.response_status
    }

    /// Sets the default [ResponseStatus](ssp::ResponseStatus).
    pub fn set_response_status(&mut self, response_status: ssp::ResponseStatus) {
        self.response_status = response_status;
    }

    /// Gets a default response message based on the [MessageType](ssp::MessageType).
    pub fn default_response(msg_type: ssp::MessageType) -> Option<Box<dyn ResponseOps>> {
        match msg_type {
            ssp::MessageType::SetInhibits => Some(Box::new(ssp::SetInhibitsResponse::new())),
            ssp::MessageType::DisplayOn => Some(Box::new(ssp::DisplayOnResponse::new())),
            ssp::MessageType::DisplayOff => Some(Box::new(ssp::DisplayOffResponse::new())),
            ssp::MessageType::SetupRequest => Some(Box::new(ssp::SetupRequestResponse::new())),
            ssp::MessageType::HostProtocolVersion => {
                Some(Box::new(ssp::HostProtocolVersionResponse::new()))
            }
            ssp::MessageType::Poll => Some(Box::new(ssp::PollResponse::new())),
            ssp::MessageType::Reject => Some(Box::new(ssp::RejectResponse::new())),
            ssp::MessageType::Disable => Some(Box::new(ssp::DisableResponse::new())),
            ssp::MessageType::Enable => Some(Box::new(ssp::EnableResponse::new())),
            ssp::MessageType::SerialNumber => Some(Box::new(ssp::SerialNumberResponse::new())),
            ssp::MessageType::UnitData => Some(Box::new(ssp::UnitDataResponse::new())),
            ssp::MessageType::ChannelValueData => {
                Some(Box::new(ssp::ChannelValueDataResponse::new()))
            }
            ssp::MessageType::Synchronisation => Some(Box::new(ssp::PollResponse::new())),
            ssp::MessageType::LastRejectCode => Some(Box::new(ssp::LastRejectCodeResponse::new())),
            ssp::MessageType::Hold => Some(Box::new(ssp::HoldResponse::new())),
            ssp::MessageType::DatasetVersion => Some(Box::new(ssp::DatasetVersionResponse::new())),
            ssp::MessageType::SetBarcodeReaderConfiguration => {
                Some(Box::new(ssp::SetBarcodeReaderConfigurationResponse::new()))
            }
            ssp::MessageType::GetBarcodeReaderConfiguration => {
                Some(Box::new(ssp::GetBarcodeReaderConfigurationResponse::new()))
            }
            ssp::MessageType::GetBarcodeInhibit => {
                Some(Box::new(ssp::GetBarcodeInhibitResponse::new()))
            }
            ssp::MessageType::SetBarcodeInhibit => {
                Some(Box::new(ssp::SetBarcodeInhibitResponse::new()))
            }
            ssp::MessageType::GetBarcodeData => Some(Box::new(ssp::GetBarcodeDataResponse::new())),
            ssp::MessageType::Empty => Some(Box::new(ssp::EmptyResponse::new())),
            ssp::MessageType::PayoutByDenomination => {
                Some(Box::new(ssp::PayoutByDenominationResponse::new()))
            }
            ssp::MessageType::SmartEmpty => Some(Box::new(ssp::SmartEmptyResponse::new())),
            ssp::MessageType::ConfigureBezel => Some(Box::new(ssp::ConfigureBezelResponse::new())),
            ssp::MessageType::PollWithAck => Some(Box::new(ssp::PollWithAckResponse::new())),
            ssp::MessageType::EventAck => Some(Box::new(ssp::EventAckResponse::new())),
            ssp::MessageType::DisablePayout => Some(Box::new(ssp::DisablePayoutResponse::new())),
            ssp::MessageType::EnablePayout => Some(Box::new(ssp::EnablePayoutResponse::new())),
            // FIXME: add support for encrypted message types...
            _ => None,
        }
    }

    /// Sends a default response message over the serial connection.
    pub fn send_default_response(&mut self) -> Result<()> {
        if let Some(mut res) = Self::default_response(self.msg_type) {
            res.set_response_status(self.response_status);
            self.serial_port.write_all(res.as_bytes())?;
        }
        Ok(())
    }

    /// Starts a device server loop
    ///
    /// The loop will continue serving configured responses to received message until the `stop`
    /// flag is set to true.
    ///
    /// # Parameters
    ///
    /// - `stop`: atomic flag for stopping the device server loop.
    pub fn serve(&mut self, stop: Arc<AtomicBool>) -> Result<()> {
        let mut buf = [0u8; ssp::len::MAX_MESSAGE];
        while !stop.load(Ordering::Relaxed) {
            if self.serial_port.bytes_to_read().unwrap_or(0) > 0
                && self
                    .serial_port
                    .read_exact(&mut buf[..=ssp::message::index::LEN])
                    .is_ok()
            {
                log::debug!("Received host message");
                let len = buf[ssp::message::index::LEN] as usize;
                let rem = len + ssp::message::index::LEN + 2;

                self.serial_port
                    .read_exact(&mut buf[ssp::message::index::LEN..rem])?;

                log::debug!("Receive message bytes: {:x?}", &buf[..rem]);

                let msg_type: ssp::MessageType = if len > ssp::message::index::DATA {
                    buf[ssp::message::index::DATA].into()
                } else {
                    self.msg_type
                };

                let seq_id = !ssp::SequenceId::from(buf[ssp::message::index::SEQ_ID]);

                if let Some(mut res) = Self::default_response(msg_type) {
                    res.set_response_status(self.response_status);
                    res.set_data_len(1);
                    res.set_sequence_id(seq_id);
                    log::debug!("Writing response: {:x?}", res.as_bytes());
                    self.serial_port.write_all(res.as_bytes())?;
                    log::trace!("Successfully wrote response");
                    self.serial_port.flush()?;
                }
            }
            buf.iter_mut().for_each(|b| *b = 0);
        }
        Ok(())
    }
}
