//! Holds private implementations of [DeviceHandle] functionality.

use std::sync::mpsc;

use ssp::MessageOps;

use crate::continue_on_err;

use super::{set_escrowed, set_escrowed_amount, DeviceHandle};

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
            match ssp::ResponseStatus::from(data[idx]) {
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
                        set_escrowed_amount(*value);
                    }

                    continue_on_err!(
                        tx.send(ssp::Event::from(event)),
                        "Failed to send Read event"
                    );
                }
                ssp::ResponseStatus::NoteCredit => {
                    let event = continue_on_err!(
                        ssp::NoteCreditEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse NoteCredit event"
                    );
                    idx += ssp::NoteCreditEvent::len();

                    // Bill moved from escrow to storage, modify global escrow state.
                    set_escrowed(false);
                    set_escrowed_amount(*event.value());

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
                    continue_on_err!(
                        tx.send(ssp::Event::from(event)),
                        "Failed to send CashboxRemoved event"
                    );
                }
                ssp::ResponseStatus::CashboxReplaced => {
                    let event = continue_on_err!(
                        ssp::CashboxReplacedEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse CashboxReplaced event"
                    );
                    idx += ssp::CashboxReplacedEvent::len();
                    continue_on_err!(
                        tx.send(ssp::Event::from(event)),
                        "Failed to send CashboxReplaced event"
                    );
                }
                ssp::ResponseStatus::Disabled => {
                    let event = continue_on_err!(
                        ssp::DisabledEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse Disabled event"
                    );
                    idx += ssp::DisabledEvent::len();
                    continue_on_err!(
                        tx.send(ssp::Event::from(event)),
                        "Failed to send Disabled event"
                    );
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

                    set_escrowed(false);

                    continue_on_err!(
                        tx.send(ssp::Event::from(event)),
                        "Failed to send Rejected event"
                    );
                }
                ssp::ResponseStatus::Rejecting => {
                    let event = continue_on_err!(
                        ssp::RejectingEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse Rejecting event"
                    );
                    idx += ssp::RejectingEvent::len();

                    set_escrowed(false);

                    continue_on_err!(
                        tx.send(ssp::Event::from(event)),
                        "Failed to send Rejecting event"
                    );
                }
                ssp::ResponseStatus::Stacked => {
                    let event = continue_on_err!(
                        ssp::StackedEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse Stacked event"
                    );
                    idx += ssp::StackedEvent::len();

                    set_escrowed(false);

                    continue_on_err!(
                        tx.send(ssp::Event::from(event)),
                        "Failed to send Stacked event"
                    );
                }
                ssp::ResponseStatus::StackerFull => {
                    let event = continue_on_err!(
                        ssp::StackerFullEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse StackerFull event"
                    );
                    continue_on_err!(
                        tx.send(ssp::Event::from(event)),
                        "Failed to send StackerFull event"
                    );
                    idx += ssp::StackerFullEvent::len();
                }
                ssp::ResponseStatus::Stacking => {
                    let event = continue_on_err!(
                        ssp::StackingEvent::try_from(data[idx..].as_ref()),
                        "Failed to parse Stacking event"
                    );
                    idx += ssp::StackingEvent::len();

                    set_escrowed(false);

                    continue_on_err!(
                        tx.send(ssp::Event::from(event)),
                        "Failed to send Stacking event"
                    );
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
