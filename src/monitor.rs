#![deny(missing_docs)]
#![doc(html_root_url = "http://haimgel.github.io/ddc-macos-rs/")]

//! Implementation of DDC/CI traits on MacOS.
//!
//! # Example
//!
//! ```rust,no_run
//! extern crate ddc;
//! extern crate ddc_macos;
//!
//! # fn main() {
//! use ddc::Ddc;
//! use ddc_macos::Monitor;
//!
//! for mut ddc in Monitor::enumerate().unwrap() {
//!     let input = ddc.get_vcp_feature(0x60).unwrap();
//!     println!("Current input: {:04x}", input.value());
//! }
//! # }
//! ```

use crate::iokit_io2c_interface::*;
use core_foundation::base::{kCFAllocatorDefault, CFType, TCFType};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::display::{CGDisplay, CGError};
use ddc::{
    Command, CommandResult, DdcCommand, DdcCommandMarker, DdcHost, ErrorCode, I2C_ADDRESS_DDC_CI, SUB_ADDRESS_DDC_CI,
};
use mach::kern_return::{kern_return_t, KERN_FAILURE};
use mach::port::MACH_PORT_NULL;
use std::os::raw::c_char;
use std::{fmt, iter};
use IOKit_sys::{
    io_iterator_t, io_service_t, kIOMasterPortDefault, kIOReturnSuccess, kMillisecondScale, IOItemCount,
    IOIteratorNext, IOObjectRelease, IOObjectRetain, IOOptionBits, IORegistryEntryCreateCFProperties,
    IOServiceGetMatchingServices, IOServiceMatching, IOServiceNameMatching,
};

extern "C" {
    #[link(name = "IOKit", kind = "framework")]
    fn IODisplayCreateInfoDictionary(framebuffer: io_service_t, options: IOOptionBits) -> CFDictionaryRef;
}

/// An error that can occur during DDC/CI communication with a monitor
#[derive(Debug)]
pub enum Error {
    /// Core Graphics errors
    CoreGraphics(CGError),
    /// Kernel I/O errors
    Io(kern_return_t),
    /// DDC/CI errors
    Ddc(ErrorCode),
}

fn verify_io(result: kern_return_t) -> Result<(), Error> {
    return if result == kIOReturnSuccess {
        Ok(())
    } else {
        Err(Error::Io(result))
    };
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
    frame_buffer: io_service_t,
}

impl Drop for Monitor {
    fn drop(&mut self) {
        unsafe {
            IOObjectRelease(self.frame_buffer);
        }
    }
}

impl fmt::Display for Monitor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl Monitor {
    /// Create a new monitor from the specified handle.
    fn new(monitor: CGDisplay, frame_buffer: io_service_t) -> Self {
        Monitor { monitor, frame_buffer }
    }

    /// Enumerate all connected physical monitors.
    pub fn enumerate() -> std::result::Result<Vec<Self>, Error> {
        unsafe {
            let displays = CGDisplay::active_displays()
                .map_err(|x| Error::from(x))?
                .into_iter()
                .map(|display_id| {
                    let display = CGDisplay::new(display_id);
                    let frame_buffer = Self::get_io_framebuffer_port(display)?;
                    Some(Self::new(display, frame_buffer))
                })
                .filter_map(|x| x)
                .collect();
            Ok(displays)
        }
    }

    /// Physical monitor description string.
    pub fn description(&self) -> String {
        format!("{:?}", self.monitor)
    }

    /// CoreGraphics display handle for this monitor
    pub fn handle(&self) -> CGDisplay {
        self.monitor
    }

    unsafe fn framebuffer_port_matches_display(port: io_service_t, display: CGDisplay) -> Option<()> {
        const NO_PRODUCT_NAME: IOOptionBits = 0x00000400;

        let mut bus_count: IOItemCount = 0;
        IOFBGetI2CInterfaceCount(port, &mut bus_count);
        if bus_count == 0 {
            return None;
        };

        let info = IODisplayCreateInfoDictionary(port, NO_PRODUCT_NAME).as_ref()?;
        let info = CFDictionary::<CFString, CFType>::wrap_under_create_rule(info);

        let display_vendor_key = CFString::from_static_string("DisplayVendorID");
        let display_product_key = CFString::from_static_string("DisplayProductID");
        let display_serial_key = CFString::from_static_string("DisplaySerialNumber");

        let display_vendor = info.find(display_vendor_key)?.downcast::<CFNumber>()?.to_i64()? as u32;
        let display_product = info.find(display_product_key)?.downcast::<CFNumber>()?.to_i64()? as u32;
        let display_serial = info.find(display_serial_key)?.downcast::<CFNumber>()?.to_i64()? as u32;

        if display_vendor == display.vendor_number()
            && display_product == display.model_number()
            && display_serial == display.serial_number()
        {
            return Some(());
        }
        return None;
    }

    unsafe fn get_io_framebuffer_port(display: CGDisplay) -> Option<io_service_t> {
        if display.is_builtin() {
            return None;
        }
        let mut iter: io_iterator_t = 0;
        let io_framebuffer = b"IOFramebuffer\0".as_ptr() as *const c_char;

        if IOServiceGetMatchingServices(kIOMasterPortDefault, IOServiceMatching(io_framebuffer), &mut iter)
            == kIOReturnSuccess
        {
            defer! { IOObjectRelease(iter); };

            let mut serv: io_service_t;
            while (serv = IOIteratorNext(iter), serv).1 != MACH_PORT_NULL {
                defer! { IOObjectRelease(serv); };

                if Self::framebuffer_port_matches_display(serv, display).is_some() {
                    IOObjectRetain(serv);
                    return Some(serv);
                }
            }
        }
        return None;
    }

    /// Get supported I2C / DDC transaction types
    /// DDCciReply is what we want, but Simple will also work
    unsafe fn get_supported_transaction_type() -> Option<u32> {
        let mut iter: io_iterator_t = 0;
        let mut io_service: io_service_t;

        let transaction_types_key = CFString::from_static_string("IOI2CTransactionTypes");
        let framebuffer_interface_name = std::ffi::CStr::from_bytes_with_nul_unchecked(b"IOFramebufferI2CInterface\0");

        if IOServiceGetMatchingServices(
            kIOMasterPortDefault,
            IOServiceNameMatching(framebuffer_interface_name.as_ptr()),
            &mut iter,
        ) == kIOReturnSuccess
        {
            defer! { IOObjectRelease(iter); };
            while (io_service = IOIteratorNext(iter), io_service).1 != MACH_PORT_NULL {
                defer! { IOObjectRelease(io_service); };
                let mut service_properties = std::ptr::null_mut();

                if IORegistryEntryCreateCFProperties(io_service, &mut service_properties, kCFAllocatorDefault as _, 0)
                    == kIOReturnSuccess
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
        }
        return None;
    }

    /// send an I2C request to a display
    unsafe fn send_request(&self, request: &mut IOI2CRequest, post_request_delay: u32) -> Result<(), Error> {
        let mut bus_count: io_service_t = 0;
        let mut result: kern_return_t = KERN_FAILURE;
        verify_io(IOFBGetI2CInterfaceCount(self.frame_buffer, &mut bus_count))?;
        for bus in 0..bus_count {
            let mut interface: io_service_t = 0;
            if IOFBCopyI2CInterfaceForBus(self.frame_buffer, bus, &mut interface) == kIOReturnSuccess {
                defer! { IOObjectRelease(interface); };
                let mut connect: IOI2CConnectRef = 0;
                if IOI2CInterfaceOpen(interface, 0, &mut connect) == kIOReturnSuccess {
                    defer! { IOI2CInterfaceClose(connect, 0); };
                    if IOI2CSendRequest(connect, 0, request) == kIOReturnSuccess {
                        result = request.result;
                        break;
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(post_request_delay as u64));
        verify_io(result)?;
        Ok(())
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
    fn execute<C: Command>(&mut self, command: C) -> std::result::Result<<C as Command>::Ok, Self::Error> {
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
        ddc::CommandResult::decode(&reply_data[2..reply_length + 2]).map_err(From::from)
    }
}

impl DdcCommandMarker for Monitor {}
