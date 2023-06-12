# SSP server

This crate contains a reference server implementing the SSP protocol for ITL devices.

# Running tests

The end-to-end tests require a connected device that supports the SSP/eSSP protocol.

The device should be available over a serial port, e.g. `/dev/ttyUSB0`.

Once the device is connected:

```
# Run the end-to-end tests
cargo test --features test-e2e

# Run an example test that cycles all the RGB bezel settings
cargo test --features test-rainbow
```
