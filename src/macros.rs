/// Convenience macro to get a Option<&ssp::AesKey> from the [DeviceHandle](crate::DeviceHandle).
///
/// If the encryption key is unset, returns `None`.
#[macro_export]
macro_rules! encryption_key {
    ($handle:tt) => {{
        $handle.encryption_key()?.as_ref()
    }};
}

/// Continues to next loop iteration on an `Err(_)` result.
#[macro_export]
macro_rules! continue_on_err {
    ($res:expr, $err:tt) => {{
        match $res {
            Ok(res) => res,
            Err(err) => {
                let err_msg = $err;
                log::warn!("{err_msg}: {err}");
                continue;
            }
        }
    }};
}
