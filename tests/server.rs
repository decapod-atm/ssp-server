#[cfg(any(feature = "test-e2e", feature = "test-crypto"))]
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread, time,
};

mod common;

#[cfg(feature = "test-e2e")]
#[test]
fn test_e2e_server() -> Result<(), Vec<ssp::Error>> {
    common::init();

    let stop_polling = Arc::new(AtomicBool::new(false));

    let mut handle = match ssp_server::DeviceHandle::new("/dev/ttyUSB0") {
        Ok(h) => h,
        Err(err) => return Err(vec![err]),
    };

    let mut errs = Vec::with_capacity(48);

    match handle.enable() {
        Ok(res) => log::debug!("Enable command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed enable command: {err}");
            errs.push(err);
        }
    }

    match handle.start_background_polling(Arc::clone(&stop_polling)) {
        Ok(_) => log::debug!("Background polling initialization successful."),
        Err(err) => log::error!("Background polling initialization failed: {err}"),
    }

    match handle.poll() {
        Ok(res) => log::debug!("Poll command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed poll command: {err}");
            errs.push(err);
        }
    }

    match handle.display_off() {
        Ok(res) => log::debug!("Display off command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed display off command: {err}");
            errs.push(err);
        }
    };

    match handle.display_on() {
        Ok(res) => log::debug!("Display on command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed display on command: {err}");
            errs.push(err);
        }
    };

    match handle.host_protocol_version(ssp::ProtocolVersion::Eight) {
        Ok(res) => log::debug!("Host protocol version command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed host protocol version command: {err}");
            errs.push(err);
        }
    };

    match handle.reject() {
        Ok(res) => log::debug!("Reject command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed reject command: {err}");
            errs.push(err);
        }
    };

    match handle.serial_number() {
        Ok(res) => log::debug!("Serial number command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed serial number command: {err}");
            errs.push(err);
        }
    };

    match handle.setup_request() {
        Ok(res) => log::debug!("Setup request command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed setup request command: {err}");
            errs.push(err);
        }
    };

    match handle.unit_data() {
        Ok(res) => log::debug!("Unit data command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed unit data command: {err}");
            errs.push(err);
        }
    };

    match handle.channel_value_data() {
        Ok(res) => log::debug!("Channel value data command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed channel value data command: {err}");
            errs.push(err);
        }
    };

    match handle.last_reject_code() {
        Ok(res) => log::debug!("Last reject code command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed last reject code command: {err}");
            errs.push(err);
        }
    };

    match handle.hold() {
        Ok(res) => log::debug!("Hold command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed hold command: {err}");
            errs.push(err);
        }
    };

    let has_barcode_reader = match handle.has_barcode_reader() {
        Ok(r) => {
            log::debug!("Has barcode reader: {r}");
            r
        }
        Err(err) => {
            errs.push(err);
            false
        }
    };

    match handle.get_barcode_reader_configuration() {
        Ok(res) => log::debug!("Get barcode reader configuration command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed get barcode reader configuration command: {err}");
            errs.push(err);
        }
    };

    let mut barcode_config = ssp::BarcodeConfiguration::default();
    barcode_config.set_enabled_status(ssp::BarcodeEnabledStatus::Top);
    barcode_config.set_num_characters(ssp::BarcodeCharacters::from(24));

    match handle.set_barcode_reader_configuration(barcode_config) {
        Ok(res) => {
            log::debug!("Set barcode reader configuration command succeeded: {res}");
            if has_barcode_reader {
                assert_eq!(res.configuration(), barcode_config);
            }
        }
        Err(err) => {
            log::error!("Failed set barcode reader configuration command: {err}");
            errs.push(err);
        }
    };

    let enable_list = ssp::EnableBitfieldList::from([ssp::EnableBitfield::from(0xff)]);

    match handle.set_inhibits(enable_list) {
        Ok(res) => {
            log::debug!("Set inhibits command for 8 channels succeeded: {res}");
        }
        Err(err) => {
            log::error!("Failed set inhibits command for 8 channels: {err}");
            errs.push(err);
        }
    }

    let enable_list = ssp::EnableBitfieldList::from([
        ssp::EnableBitfield::from(0xff),
        ssp::EnableBitfield::from(0xff),
    ]);

    match handle.set_inhibits(enable_list) {
        Ok(res) => {
            log::debug!("Set inhibits command for 16 channels succeeded: {res}");
        }
        Err(err) => {
            log::error!("Failed set inhibits command for 16 channels: {err}");
            errs.push(err);
        }
    }

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
            errs.push(err);
        }
    }

    /* Documentation says NV200/SMART Payout supports up to 64 channels,
     * but only 8-24 channels return successful response statuses.
    let enable_list = ssp::EnableBitfieldList::from([
        ssp::EnableBitfield::from(0xff),
        ssp::EnableBitfield::from(0xff),
        ssp::EnableBitfield::from(0xff),
        ssp::EnableBitfield::from(0xff),
        ssp::EnableBitfield::from(0xff),
        ssp::EnableBitfield::from(0xff),
        ssp::EnableBitfield::from(0xff),
        ssp::EnableBitfield::from(0xff),
    ]);

    match handle.set_inhibits(enable_list) {
        Ok(res) => {
            log::debug!("Set inhibits command for 64 channels succeeded: {res}");
        }
        Err(err) => {
            log::error!("Failed set inhibits command for 64 channels: {err}");
            errs.push(err);
        }
    }
    */

    match handle.get_barcode_inhibit() {
        Ok(res) => {
            log::debug!("Get barcode inhibit command succeeded: {res}");
        }
        Err(err) => {
            log::error!("Failed get barcode inhibit command: {err}");
            errs.push(err);
        }
    };

    let mut inhibit = ssp::BarcodeCurrencyInhibit::new();

    inhibit.set_currency_enabled(ssp::CurrencyEnabled::Set);
    inhibit.set_barcode_enabled(ssp::BarcodeEnabled::Set);

    inhibit.set_channel_0_enabled(ssp::ChannelEnabled::Set);
    inhibit.set_channel_1_enabled(ssp::ChannelEnabled::Set);
    inhibit.set_channel_2_enabled(ssp::ChannelEnabled::Set);
    inhibit.set_channel_3_enabled(ssp::ChannelEnabled::Set);
    inhibit.set_channel_4_enabled(ssp::ChannelEnabled::Set);
    inhibit.set_channel_5_enabled(ssp::ChannelEnabled::Set);

    match handle.set_barcode_inhibit(inhibit) {
        Ok(res) => {
            log::debug!("Set barcode inhibit command succeeded: {res}");
        }
        Err(err) => {
            log::error!("Failed set barcode inhibit command: {err}");
            errs.push(err);
        }
    };

    match handle.get_barcode_reader_configuration() {
        Ok(res) => {
            log::debug!(
                "Get barcode reader configuration command succeeded (after setting): {res}"
            );
            if has_barcode_reader {
                assert_eq!(res.configuration(), barcode_config);
            }
        }
        Err(err) => {
            log::error!("Failed get barcode reader configuration command: {err}");
            errs.push(err);
        }
    };

    match handle.get_barcode_data() {
        Ok(res) => {
            log::debug!("Get barcode data command succeeded: {res}");
        }
        Err(err) => {
            log::error!("Failed get barcode data command: {err}");
            errs.push(err);
        }
    };

    match handle.configure_bezel(
        ssp::RGB::from([0xf2, 0xa9, 0x00]),
        ssp::BezelConfigStorage::Ram,
    ) {
        Ok(res) => {
            log::debug!("Configure bezel command succeeded: {res}");
        }
        Err(err) => {
            log::error!("Failed configure bezel command: {err}");
            errs.push(err);
        }
    };

    match handle.poll_with_ack() {
        Ok(res) => log::debug!("Poll with ack command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed poll with ack command: {err}");
            errs.push(err);
        }
    }

    match handle.event_ack() {
        Ok(res) => log::debug!("Event ack command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed event ack command: {err}");
            errs.push(err);
        }
    }

    // Perform key negotiation sequence

    // Reset the encryption key to default
    match handle.encryption_reset() {
        Ok(res) => log::debug!("Encryption reset command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed encryption reset command: {err}");
            errs.push(err);
        }
    }

    match handle.set_generator() {
        Ok(res) => log::debug!("Set generator command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed set generator command: {err}");
            errs.push(err);
        }
    }

    match handle.set_modulus() {
        Ok(res) => log::debug!("Set modulus command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed set modulus command: {err}");
            errs.push(err);
        }
    }

    match handle.request_key_exchange() {
        Ok(res) => log::debug!("Request key exchange command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed request key exchange command: {err}");
            errs.push(err);
        }
    }

    match handle.set_encryption_key() {
        Ok(res) => log::debug!("Set encryption key command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed set encryption key command: {err}");
            errs.push(err);
        }
    }

    // Send messages that require encryption

    match handle.empty() {
        Ok(res) => log::debug!("Empty command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed empty command: {err}");
            errs.push(err);
        }
    };

    match handle.smart_empty() {
        Ok(res) => log::debug!("Smart empty command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed smart empty command: {err}");
            errs.push(err);
        }
    };

    match handle.disable() {
        Ok(res) => log::debug!("Disable command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed disable command: {err}");
            errs.push(err);
        }
    }

    stop_polling.store(true, Ordering::SeqCst);

    match handle.reset() {
        Ok(_) => log::debug!("Reset command succeeded"),
        Err(err) => {
            log::error!("Failed reset command: {err}");
            errs.push(err);
        }
    }

    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

#[cfg(feature = "test-rainbow")]
#[test]
fn test_rainbow_dance() -> ssp::Result<()> {
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread, time,
    };

    common::init();

    let mut handle = ssp_server::DeviceHandle::new("/dev/ttyUSB0")?;

    let stop_polling = Arc::new(AtomicBool::new(false));

    handle.host_protocol_version(ssp::ProtocolVersion::Eight)?;

    match handle.setup_request() {
        Ok(setup) => log::info!("Device status: {setup}"),
        Err(err) => log::error!("Error retrieving device status: {err}"),
    }

    match handle.serial_number() {
        Ok(sn) => log::info!("Device serial number: {sn}"),
        Err(err) => log::error!("Error retrieving device status: {err}"),
    }

    handle.start_background_polling(Arc::clone(&stop_polling))?;

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

    let start = 0;
    let end = 255;
    let red_step = 32;
    let green_step = 32;
    let blue_step = 32;
    let delay = 10;

    let range = start..=end;

    let now = time::Instant::now();

    let ram = ssp::BezelConfigStorage::Ram;

    for b in range.clone().step_by(blue_step) {
        for g in range.clone().step_by(green_step) {
            for r in range.clone().step_by(red_step) {
                if now.elapsed().as_millis() % 200 == 0 {
                    let poll_response = handle.poll()?;
                    log::debug!("Poll response: {poll_response}");
                }

                thread::sleep(time::Duration::from_millis(delay));

                let rgb = ssp::RGB::from([r, g, b]);

                if let Err(err) = handle.configure_bezel(rgb, ram) {
                    log::error!("Configure bezel command failed: [{rgb}], error: {err}");
                    return Err(err);
                }

                if let Err(err) = handle.display_on() {
                    log::error!("Display on command failed: [{rgb}], error: {err}");
                    return Err(err);
                }
            }
        }
    }

    thread::sleep(time::Duration::from_millis(delay));

    stop_polling.store(true, Ordering::SeqCst);

    handle.reset()?;

    Ok(())
}

#[test]
#[cfg(feature = "test-crypto")]
fn test_crypto_server() -> Result<(), Vec<ssp::Error>> {
    common::init();

    let stop_polling = Arc::new(AtomicBool::new(false));

    let mut handle = match ssp_server::DeviceHandle::new("/dev/ttyUSB0") {
        Ok(h) => h,
        Err(err) => return Err(vec![err]),
    };

    let mut errs = Vec::with_capacity(48);

    match handle.sync() {
        Ok(res) => log::debug!("Sync command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed sync command: {err}");
            errs.push(err);
        }
    }

    match handle.sync() {
        Ok(res) => log::debug!("Sync command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed sync command: {err}");
            errs.push(err);
        }
    }

    match handle.set_generator() {
        Ok(res) => log::debug!("Set generator command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed set generator command: {err}");
            errs.push(err);
        }
    }

    match handle.set_modulus() {
        Ok(res) => log::debug!("Set modulus command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed set modulus command: {err}");
            errs.push(err);
        }
    }

    match handle.request_key_exchange() {
        Ok(res) => log::debug!("Request key exchange command succeeded: {res}"),
        Err(err) => {
            log::error!("Failed request key exchange command: {err}");
            errs.push(err);
        }
    }

    thread::sleep(time::Duration::from_millis(500));

    // Send messages that require encryption

    match handle.enable_device(ssp::ProtocolVersion::Six) {
        Ok(res) => log::debug!("Enable device succeeded: {res}"),
        Err(err) => {
            log::error!("Enable device failed: {err}");
            errs.push(err);
        }
    };

    // Reset the device

    match handle.full_reset() {
        Ok(_res) => log::debug!("Reset command succeeded"),
        Err(err) => {
            log::error!("Failed reset command: {err}");
            errs.push(err);
        }
    }

    stop_polling.store(true, Ordering::SeqCst);

    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}
