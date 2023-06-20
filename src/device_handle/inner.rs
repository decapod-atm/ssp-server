//! Holds private implementations of [DeviceHandle] functionality.

use std::sync::mpsc;

use ssp::MessageOps;

use crate::continue_on_err;

use super::{
    cashbox_available, set_cashbox_available, set_escrowed, set_escrowed_amount, DeviceHandle,
};

impl DeviceHandle {
    pub(crate) fn parse_events(
        poll_res: &ssp::PollResponse,
        tx: &mpsc::Sender<ssp::Event>,
    ) -> ssp::Result<()> {
        let data = poll_res.data();
        let data_len = data.len();
        let mut idx = 1;

        // Usually, only one event is returned during normal polling.
        //
        // Just in case multiple events are returned, keep parsing to the end of the data array.
        while idx < data_len {
            let status = ssp::ResponseStatus::from(data[idx]);

            if !(cashbox_available()
                || status == ssp::ResponseStatus::Disabled
                || status == ssp::ResponseStatus::StackerFull
                || status == ssp::ResponseStatus::CashboxRemoved)
            {
                set_cashbox_available(true);

                log::debug!("Cashbox is available: {status}");

                continue_on_err!(
                    tx.send(ssp::Event::from(ssp::CashboxReplacedEvent::new())),
                    "Failed to send CashboxReplaced event"
                );
            }

            match status {
                ssp::ResponseStatus::DeviceReset => {
                    let event = continue_on_err!(
                        ssp::ResetEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse Reset event"
                    );
                    idx += ssp::ResetEvent::len();
                    continue_on_err!(
                        tx.send(ssp::Event::from(event)),
                        "Failed to send Reset event"
                    );
                }
                ssp::ResponseStatus::Read => {
                    let event = continue_on_err!(
                        ssp::ReadEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse Read event"
                    );
                    idx += ssp::ReadEvent::len();

                    // A ReadEvent with a non-zero value means the document has moved into escrow.
                    let value = event.value();
                    if value.as_inner() != 0 {
                        // Change the global escrow state
                        set_escrowed(true);
                        set_escrowed_amount(value);

                        continue_on_err!(
                            tx.send(ssp::Event::from(event)),
                            "Failed to send Read event"
                        );
                    }
                }
                ssp::ResponseStatus::NoteCredit => {
                    let event = continue_on_err!(
                        ssp::NoteCreditEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse NoteCredit event"
                    );
                    idx += ssp::NoteCreditEvent::len();

                    // Bill moved from escrow to storage, modify global escrow state.
                    set_escrowed(false);
                    set_escrowed_amount(event.value());

                    continue_on_err!(
                        tx.send(ssp::Event::from(event)),
                        "Failed to send NoteCredit event"
                    );
                }
                ssp::ResponseStatus::CashboxRemoved => {
                    let event = continue_on_err!(
                        ssp::CashboxRemovedEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse CashboxRemoved event"
                    );

                    idx += ssp::CashboxRemovedEvent::len();

                    if cashbox_available() {
                        log::debug!("Cashbox is removed");

                        set_cashbox_available(false);

                        continue_on_err!(
                            tx.send(ssp::Event::from(event)),
                            "Failed to send CashboxRemoved event"
                        );
                    }
                }
                ssp::ResponseStatus::CashboxReplaced => {
                    let event = continue_on_err!(
                        ssp::CashboxReplacedEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse CashboxReplaced event"
                    );

                    idx += ssp::CashboxReplacedEvent::len();

                    if !cashbox_available() {
                        log::debug!("Cashbox replaced");

                        set_cashbox_available(true);

                        continue_on_err!(
                            tx.send(ssp::Event::from(event)),
                            "Failed to send CashboxReplaced event"
                        );
                    }
                }
                ssp::ResponseStatus::Disabled => {
                    let event = continue_on_err!(
                        ssp::DisabledEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse Disabled event"
                    );
                    log::trace!("Device is disabled: {event}");
                    idx += ssp::DisabledEvent::len();
                }
                ssp::ResponseStatus::FraudAttempt => {
                    let event = continue_on_err!(
                        ssp::FraudAttemptEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse FraudAttempt event"
                    );
                    idx += ssp::FraudAttemptEvent::len();
                    continue_on_err!(
                        tx.send(ssp::Event::from(event)),
                        "Failed to send FraudAttempt event"
                    );
                }
                ssp::ResponseStatus::NoteClearedFromFront => {
                    let event = continue_on_err!(
                        ssp::NoteClearedFromFrontEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse NoteClearedFromFront event"
                    );
                    idx += ssp::NoteClearedFromFrontEvent::len();
                    continue_on_err!(
                        tx.send(ssp::Event::from(event)),
                        "Failed to send NoteClearedFromFront event"
                    );
                }
                ssp::ResponseStatus::NoteClearedIntoCashbox => {
                    let event = continue_on_err!(
                        ssp::NoteClearedIntoCashboxEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse NoteClearedIntoCashbox event"
                    );
                    idx += ssp::NoteClearedIntoCashboxEvent::len();
                    continue_on_err!(
                        tx.send(ssp::Event::from(event)),
                        "Failed to send NoteClearedIntoCashbox event"
                    );
                }
                ssp::ResponseStatus::Rejected => {
                    let event = continue_on_err!(
                        ssp::RejectedEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse Rejected event"
                    );
                    idx += ssp::RejectedEvent::len();

                    log::trace!("Received Rejected event: {event}");

                    set_escrowed(false);
                }
                ssp::ResponseStatus::Rejecting => {
                    let event = continue_on_err!(
                        ssp::RejectingEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse Rejecting event"
                    );
                    idx += ssp::RejectingEvent::len();

                    log::trace!("Received Rejecting event: {event}");

                    set_escrowed(false);
                }
                ssp::ResponseStatus::Stacked => {
                    let event = continue_on_err!(
                        ssp::StackedEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse Stacked event"
                    );
                    idx += ssp::StackedEvent::len();

                    log::trace!("Received Stacked event: {event}");

                    set_escrowed(false);
                }
                ssp::ResponseStatus::StackerFull => {
                    let event = continue_on_err!(
                        ssp::StackerFullEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse StackerFull event"
                    );
                    idx += ssp::StackerFullEvent::len();

                    if cashbox_available() {
                        // Some firmware/protocol versions seem to send this message for the
                        // cashbox being removed, and the stacker being full. TBD.
                        log::debug!(
                            "Cashbox is unavailable. It was either removed, or the stacker is full"
                        );

                        set_cashbox_available(false);

                        continue_on_err!(
                            tx.send(ssp::Event::from(event)),
                            "Failed to send StackerFull event"
                        );
                    }
                }
                ssp::ResponseStatus::Stacking => {
                    let event = continue_on_err!(
                        ssp::StackingEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse Stacking event"
                    );
                    idx += ssp::StackingEvent::len();

                    log::trace!("Received Stacking event: {event}");

                    set_escrowed(false);
                }
                ssp::ResponseStatus::UnsafeJam => {
                    let event = continue_on_err!(
                        ssp::UnsafeJamEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse UnsafeJam event"
                    );
                    idx += ssp::UnsafeJamEvent::len();

                    set_escrowed(false);

                    continue_on_err!(
                        tx.send(ssp::Event::from(event)),
                        "Failed to send UnsafeJam event"
                    );
                }
                _ => log::warn!("Unsupported event occurred: 0x{:02x}", data[idx]),
            }
        }

        Ok(())
    }
}
