#![deny(missing_docs)]

use crate::iokit::display::*;
use crate::iokit::io2c_interface::*;
use crate::iokit::wrappers::*;
use core_foundation::base::{kCFAllocatorDefault, CFType, TCFType};
use core_foundation::data::CFData;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::display::{CGDisplay, CGError};
use ddc::{
    Command, CommandResult, DdcCommand, DdcCommandMarker, DdcHost, ErrorCode, I2C_ADDRESS_DDC_CI, SUB_ADDRESS_DDC_CI,
};
use io_kit_sys::ret::kIOReturnSuccess;
use io_kit_sys::types::{io_service_t, kMillisecondScale, IOItemCount};
use io_kit_sys::IORegistryEntryCreateCFProperties;
use mach::kern_return::{kern_return_t, KERN_FAILURE};
use std::{fmt, iter};
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
}

fn verify_io(result: kern_return_t) -> Result<(), Error> {
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

/// A handle to an attached monitor that allows the use of DDC/CI operations.
#[derive(Debug)]
pub struct Monitor {
    monitor: CGDisplay,
    frame_buffer: IoObject,
}

impl fmt::Display for Monitor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl Monitor {
    /// Create a new monitor from the specified handle.
    fn new(monitor: CGDisplay, frame_buffer: IoObject) -> Self {
        Monitor { monitor, frame_buffer }
    }

    /// Enumerate all connected physical monitors returning [Vec<Monitor>]
    pub fn enumerate() -> Result<Vec<Self>, Error> {
        let monitors = CGDisplay::active_displays()
            .map_err(Error::from)?
            .into_iter()
            .filter_map(|display_id| {
                let display = CGDisplay::new(display_id);
                let frame_buffer = Self::get_io_framebuffer_port(display)?;
                Some(Self::new(display, frame_buffer))
                })
            .collect();
        Ok(monitors)
    }

    /// Physical monitor description string. If it cannot get the product's name it will use
    /// the vendor number and model number to form a description
    pub fn description(&self) -> String {
        self.product_name().unwrap_or(format!(
            "{:04x}:{:04x}",
            self.monitor.vendor_number(),
            self.monitor.model_number()
        ))
    }

    /// Serial number for this [Monitor]
    pub fn serial_number(&self) -> Option<String> {
        let serial = self.monitor.serial_number();
        match serial {
            0 => None,
            _ => Some(format!("{}", serial))
        }
    }

    /// Product name for this [Monitor], if available
    pub fn product_name(&self) -> Option<String> {
        let info = Self::display_info_dict(&self.frame_buffer)?;
        let display_product_name_key = CFString::from_static_string("DisplayProductName");
        let display_product_names_dict = info.find(&display_product_name_key)?.downcast::<CFDictionary>()?;
        let (_, localized_product_names) = display_product_names_dict.get_keys_and_values();
        localized_product_names
            .first()
            .map(|name| unsafe { CFString::wrap_under_get_rule(*name as CFStringRef) }.to_string())
    }

    /// Returns Extended display identification data (EDID) for this [Monitor] as raw bytes data
    pub fn edid(&self) -> Option<Vec<u8>> {
        let info = Self::display_info_dict(&self.frame_buffer)?;
        let display_product_name_key = CFString::from_static_string("IODisplayEDIDOriginal");
        let edid_data = info.find(&display_product_name_key)?.downcast::<CFData>()?;
        Some(edid_data.bytes().into())
    }

    /// CoreGraphics display handle for this monitor
    pub fn handle(&self) -> CGDisplay {
        self.monitor
    }

    fn display_info_dict(frame_buffer: &IoObject) -> Option<CFDictionary<CFString, CFType>> {
        unsafe {
            let info = IODisplayCreateInfoDictionary(frame_buffer.into(), kIODisplayOnlyPreferredName).as_ref()?;
            Some(CFDictionary::<CFString, CFType>::wrap_under_create_rule(info))
        }
    }

    // Finds a framebuffer that matches display, returns a properly formatted *unique* display name
    fn framebuffer_port_matches_display(port: &IoObject, display: CGDisplay) -> Option<()> {
        let mut bus_count: IOItemCount = 0;
        unsafe {
            IOFBGetI2CInterfaceCount(port.into(), &mut bus_count);
        }
        if bus_count == 0 {
            return None;
        };

        let info = Self::display_info_dict(port)?;

        let display_vendor_key = CFString::from_static_string("DisplayVendorID");
        let display_product_key = CFString::from_static_string("DisplayProductID");
        let display_serial_key = CFString::from_static_string("DisplaySerialNumber");

        let display_vendor = info.find(&display_vendor_key)?.downcast::<CFNumber>()?.to_i64()? as u32;
        let display_product = info.find(&display_product_key)?.downcast::<CFNumber>()?.to_i64()? as u32;
        // Display serial number is not always present. If it's not there, default to zero
        // (to match what CGDisplay.serial_number() returns
        let display_serial = info
            .find(&display_serial_key)
            .and_then(|x| x.downcast::<CFNumber>())
            .and_then(|x| x.to_i32())
            .map(|x| x as u32)
            .unwrap_or(0);

        if display_vendor == display.vendor_number()
            && display_product == display.model_number()
            && display_serial == display.serial_number()
        {
            Some(())
        } else {
            None
        }
    }

    // Gets the framebuffer port
    fn get_io_framebuffer_port(display: CGDisplay) -> Option<IoObject> {
        if display.is_builtin() {
            return None;
        }
        IoIterator::for_services("IOFramebuffer")?
            .find(|framebuffer| Self::framebuffer_port_matches_display(framebuffer, display).is_some())
    }

    /// Get supported I2C / DDC transaction types
    /// DDCciReply is what we want, but Simple will also work
    unsafe fn get_supported_transaction_type() -> Option<u32> {
        let transaction_types_key = CFString::from_static_string("IOI2CTransactionTypes");

        for io_service in IoIterator::for_service_names("IOFramebufferI2CInterface")? {
            let mut service_properties = std::ptr::null_mut();
            if IORegistryEntryCreateCFProperties(
                (&io_service).into(),
                &mut service_properties,
                kCFAllocatorDefault as _,
                0,
            ) == kIOReturnSuccess
            {
                let info = CFDictionary::<CFString, CFType>::wrap_under_create_rule(service_properties as _);
                let transaction_types = info.find(&transaction_types_key)?.downcast::<CFNumber>()?.to_i64()?;
                if ((1 << kIOI2CDDCciReplyTransactionType) & transaction_types) != 0 {
                    return Some(kIOI2CDDCciReplyTransactionType);
                } else if ((1 << kIOI2CSimpleTransactionType) & transaction_types) != 0 {
                    return Some(kIOI2CSimpleTransactionType);
                }
            }
        }
        None
    }

    /// send an I2C request to a display
    unsafe fn send_request(&self, request: &mut IOI2CRequest, post_request_delay: u32) -> Result<(), Error> {
        let mut bus_count = 0;
        let mut result: Result<(), Error> = Err(Error::Io(KERN_FAILURE));
        verify_io(IOFBGetI2CInterfaceCount((&self.frame_buffer).into(), &mut bus_count))?;
        for bus in 0..bus_count {
            let mut interface: io_service_t = 0;
            if IOFBCopyI2CInterfaceForBus((&self.frame_buffer).into(), bus, &mut interface) == kIOReturnSuccess {
                let interface = IoObject::from(interface);
                result = IoI2CInterfaceConnection::new(&interface)
                    .and_then(|connection| connection.send_request(request))
                    .map_err(From::from);
                if result.is_ok() {
                    break;
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(post_request_delay as u64));
        result
    }

    fn get_response_transaction_type<C: Command>(&self, _c: C) -> u32 {
        if C::Ok::MAX_LEN == 0 {
            kIOI2CNoTransactionType
        } else {
            unsafe { Self::get_supported_transaction_type().unwrap_or(kIOI2CNoTransactionType) }
        }
    }
}

impl DdcHost for Monitor {
    type Error = Error;
}

impl DdcCommand for Monitor {
    fn execute<C: Command>(&mut self, command: C) -> Result<<C as Command>::Ok, Self::Error> {
        // Encode the command into request_data buffer
        // 36 bytes is an arbitrary number, larger than any I2C command length.
        // Cannot use [0u8; C::MAX_LEN] (associated constants do not work here)
        assert!(C::MAX_LEN <= 36);
        let mut encoded_command = [0u8; 36];
        let command_length = command.encode(&mut encoded_command)?;
        let mut request_data = [0u8; 36 + 3];
        Self::encode_command(&encoded_command[0..command_length], &mut request_data);

        // Allocate data for the response. 36 bytes is larger than any response
        assert!(C::Ok::MAX_LEN <= 36);
        let reply_data = [0u8; 36];
        let mut request: IOI2CRequest = unsafe { std::mem::zeroed() };

        request.commFlags = 0;
        request.sendAddress = (I2C_ADDRESS_DDC_CI << 1) as u32;
        request.sendTransactionType = kIOI2CSimpleTransactionType;
        request.sendBuffer = &request_data as *const _ as usize;
        request.sendBytes = (command_length + 3) as u32;
        request.minReplyDelay = C::DELAY_RESPONSE_MS * kMillisecondScale as u64;
        request.result = -1;

        request.replyTransactionType = self.get_response_transaction_type(command);
        request.replyAddress = ((I2C_ADDRESS_DDC_CI << 1) | 1) as u32;
        request.replySubAddress = SUB_ADDRESS_DDC_CI;

        request.replyBuffer = &reply_data as *const _ as usize;
        request.replyBytes = reply_data.len() as u32;

        unsafe {
            self.send_request(&mut request, C::DELAY_COMMAND_MS as u32)?;
        }

        if request.replyTransactionType == kIOI2CNoTransactionType {
            CommandResult::decode(&[0u8; 0]).map_err(From::from)
        } else {
            let reply_length = (reply_data[1] & 0x7f) as usize;
            if reply_length + 2 >= reply_data.len() {
                return Err(Error::Ddc(ErrorCode::InvalidLength));
            }

            let checksum = Self::checksum(
                iter::once(request.replyAddress as u8)
                    .chain(iter::once(SUB_ADDRESS_DDC_CI))
                    .chain(reply_data[1..2 + reply_length].iter().cloned()),
            );
            if reply_data[2 + reply_length] != checksum {
                return Err(Error::Ddc(ErrorCode::InvalidChecksum));
            }
            CommandResult::decode(&reply_data[2..reply_length + 2]).map_err(From::from)
        }
    }
}

impl DdcCommandMarker for Monitor {}
