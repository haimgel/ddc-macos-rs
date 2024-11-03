use core_graphics::base::CGError;
use ddc::ErrorCode;
use io_kit_sys::ret::kIOReturnSuccess;
use mach::kern_return::{kern_return_t, KERN_FAILURE};
use thiserror::Error;

/// An error that can occur during DDC/CI communication with a monitor
#[derive(Error, Debug)]
pub enum Error {
    /// Core Graphics errors
    #[error("Core Graphics error: {0}")]
    CoreGraphics(CGError),
    /// Kernel I/O errors
    #[error("MacOS kernel I/O error: {0}")]
    Io(kern_return_t),
    /// DDC/CI errors
    #[error("DDC/CI error: {0}")]
    Ddc(ErrorCode),
    /// Service not found
    #[error("Service not found")]
    ServiceNotFound,
    /// Display location not found
    #[error("Service not found")]
    DisplayLocationNotFound,
}

pub fn verify_io(result: kern_return_t) -> Result<(), Error> {
    if result == kIOReturnSuccess {
        Ok(())
    } else {
        Err(Error::Io(result))
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::Io(error.raw_os_error().unwrap_or(KERN_FAILURE))
    }
}

impl From<ErrorCode> for Error {
    fn from(error: ErrorCode) -> Self {
        Error::Ddc(error)
    }
}

impl From<CGError> for Error {
    fn from(error: CGError) -> Self {
        Error::CoreGraphics(error)
    }
}
