#![deny(missing_docs)]

use crate::arm::{IOAVService, IOAVServiceReadI2C, IOAVServiceWriteI2C};
use crate::error::Error;
use crate::iokit::CoreDisplay_DisplayCreateInfoDictionary;
use crate::iokit::IoObject;
use crate::iokit::*;
use crate::{arm, intel};
use core_foundation::base::{CFType, TCFType};
use core_foundation::data::CFData;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::display::CGDisplay;
use ddc::{
    Command, CommandResult, DdcCommand, DdcCommandMarker, DdcHost, ErrorCode, I2C_ADDRESS_DDC_CI, SUB_ADDRESS_DDC_CI,
};
use io_kit_sys::types::kMillisecondScale;
use std::{fmt, iter};

/// DDC access method for a monitor
#[derive(Debug)]
enum MonitorService {
    Intel(IoObject),
    Arm(IOAVService),
}

/// A handle to an attached monitor that allows the use of DDC/CI operations.
#[derive(Debug)]
pub struct Monitor {
    monitor: CGDisplay,
    service: MonitorService,
}

impl fmt::Display for Monitor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl Monitor {
    /// Create a new monitor from the specified handle.
    fn new(monitor: CGDisplay, service: MonitorService) -> Self {
        Monitor { monitor, service }
    }

    /// Enumerate all connected physical monitors returning [Vec<Monitor>]
    pub fn enumerate() -> Result<Vec<Self>, Error> {
        let monitors = CGDisplay::active_displays()
            .map_err(Error::from)?
            .into_iter()
            .filter_map(|display_id| {
                let display = CGDisplay::new(display_id);
                return if let Some(service) = intel::get_io_framebuffer_port(display) {
                    Some(Self::new(display, MonitorService::Intel(service)))
                } else if let Ok(service) = arm::get_display_av_service(display) {
                    Some(Self::new(display, MonitorService::Arm(service)))
                } else {
                    None
                };
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
            _ => Some(format!("{}", serial)),
        }
    }

    /// Product name for this [Monitor], if available
    pub fn product_name(&self) -> Option<String> {
        let info: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_create_rule(CoreDisplay_DisplayCreateInfoDictionary(self.monitor.id)) };

        let display_product_name_key = CFString::from_static_string("DisplayProductName");
        let display_product_names_dict = info.find(&display_product_name_key)?.downcast::<CFDictionary>()?;
        let (_, localized_product_names) = display_product_names_dict.get_keys_and_values();
        localized_product_names
            .first()
            .map(|name| unsafe { CFString::wrap_under_get_rule(*name as CFStringRef) }.to_string())
    }

    /// Returns Extended display identification data (EDID) for this [Monitor] as raw bytes data
    pub fn edid(&self) -> Option<Vec<u8>> {
        let info: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_create_rule(CoreDisplay_DisplayCreateInfoDictionary(self.monitor.id)) };
        let display_product_name_key = CFString::from_static_string("IODisplayEDIDOriginal");
        let edid_data = info.find(&display_product_name_key)?.downcast::<CFData>()?;
        Some(edid_data.bytes().into())
    }

    /// CoreGraphics display handle for this monitor
    pub fn handle(&self) -> CGDisplay {
        self.monitor
    }

    fn execute_intel<C: Command>(&mut self, command: C) -> Result<<C as Command>::Ok, crate::error::Error> {
        if let MonitorService::Intel(service) = &self.service {
            let request_data = Self::encode_request(&command)?;

            // Allocate data for the response. 36 bytes is larger than any response
            assert!(C::Ok::MAX_LEN <= 36);
            let reply_data = [0u8; 36];
            let mut request: IOI2CRequest = unsafe { std::mem::zeroed() };

            request.commFlags = 0;
            request.sendAddress = (I2C_ADDRESS_DDC_CI << 1) as u32;
            request.sendTransactionType = kIOI2CSimpleTransactionType;
            request.sendBuffer = &request_data as *const _ as usize;
            request.sendBytes = request_data.len() as u32;
            request.minReplyDelay = C::DELAY_RESPONSE_MS * kMillisecondScale as u64;
            request.result = -1;

            request.replyTransactionType = intel::get_response_transaction_type::<C>();
            request.replyAddress = ((I2C_ADDRESS_DDC_CI << 1) | 1) as u32;
            request.replySubAddress = SUB_ADDRESS_DDC_CI;

            request.replyBuffer = &reply_data as *const _ as usize;
            request.replyBytes = reply_data.len() as u32;

            unsafe {
                intel::send_request(service, &mut request, C::DELAY_COMMAND_MS as u32)?;
            }

            if request.replyTransactionType == kIOI2CNoTransactionType {
                CommandResult::decode(&[0u8; 0]).map_err(From::from)
            } else {
                Self::decode_reply::<C>(&reply_data)
            }
        } else {
            Err(Error::ServiceNotFound)
        }
    }

    fn execute_arm<C: Command>(&mut self, command: C) -> Result<<C as Command>::Ok, crate::error::Error> {
        if let MonitorService::Arm(service) = &self.service {
            let request_data = Self::encode_request(&command)?;
            std::thread::sleep(std::time::Duration::from_millis(C::DELAY_COMMAND_MS));
            let success = unsafe {
                IOAVServiceWriteI2C(
                    *service,
                    I2C_ADDRESS_DDC_CI as u32,
                    SUB_ADDRESS_DDC_CI as u32,
                    // Skip the first byte, which is the I2C address, which this API does not need
                    request_data[1..].as_ptr() as _,
                    (request_data.len() - 1) as _, // command_length as u32 + 3,
                )
            };
            if success != 0 {
                return Err(Error::Io(success));
            }
            if C::Ok::MAX_LEN == 0 {
                CommandResult::decode(&[0u8; 0]).map_err(From::from)
            } else {
                std::thread::sleep(std::time::Duration::from_millis(C::DELAY_RESPONSE_MS));
                // Allocate data for the response. 36 bytes is larger than any response
                assert!(C::Ok::MAX_LEN <= 36);
                let reply_data = [0u8; 36];

                let success = unsafe {
                    IOAVServiceReadI2C(
                        *service,
                        I2C_ADDRESS_DDC_CI as u32,
                        0,
                        reply_data.as_ptr() as _,
                        reply_data.len() as u32,
                    )
                };
                if success != 0 {
                    return Err(Error::Io(success));
                }
                Self::decode_reply::<C>(&reply_data)
            }
        } else {
            Err(Error::ServiceNotFound)
        }
    }

    /// Encode the command into request_data buffer
    fn encode_request<C: Command>(command: &C) -> Result<Vec<u8>, crate::error::Error> {
        // 36 bytes is an arbitrary number, larger than any I2C command length.
        // Cannot use [0u8; C::MAX_LEN] (associated constants do not work here)
        assert!(C::MAX_LEN <= 36);
        let mut encoded_command = [0u8; 36];
        let command_length = command.encode(&mut encoded_command)?;
        let mut request_data = [0u8; 36 + 3];
        Ok(Self::encode_command(&encoded_command[0..command_length], &mut request_data).to_owned())
    }

    fn decode_reply<C: Command>(reply_data: &[u8]) -> Result<<C as Command>::Ok, crate::error::Error> {
        let reply_length = (reply_data[1] & 0x7f) as usize;
        if reply_length + 2 >= reply_data.len() {
            return Err(Error::Ddc(ErrorCode::InvalidLength));
        }
        let checksum = Self::checksum(
            iter::once(((I2C_ADDRESS_DDC_CI << 1) | 1) as u8)
                .chain(iter::once(SUB_ADDRESS_DDC_CI))
                .chain(reply_data[1..2 + reply_length].iter().cloned()),
        );
        if reply_data[2 + reply_length] != checksum {
            return Err(Error::Ddc(ErrorCode::InvalidChecksum));
        }
        CommandResult::decode(&reply_data[2..C::Ok::MAX_LEN + 2]).map_err(From::from)
    }
}

impl DdcHost for Monitor {
    type Error = Error;
}

impl DdcCommand for Monitor {
    fn execute<C: Command>(&mut self, command: C) -> Result<<C as Command>::Ok, Self::Error> {
        match &self.service {
            MonitorService::Intel(_) => self.execute_intel(command),
            MonitorService::Arm(_) => self.execute_arm(command),
        }
    }
}

impl DdcCommandMarker for Monitor {}
