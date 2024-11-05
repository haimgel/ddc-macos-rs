#![deny(missing_docs)]

use crate::error::Error;
use crate::iokit::CoreDisplay_DisplayCreateInfoDictionary;
use crate::iokit::IoObject;
use crate::{arm, intel};
use core_foundation::base::{CFType, TCFType};
use core_foundation::data::CFData;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::display::CGDisplay;
use ddc::{
    DdcCommand, DdcCommandMarker, DdcCommandRaw, DdcCommandRawMarker, DdcHost, Delay, ErrorCode, I2C_ADDRESS_DDC_CI,
    SUB_ADDRESS_DDC_CI,
};
use std::time::Duration;
use std::{fmt, iter};

/// DDC access method for a monitor
#[derive(Debug)]
enum MonitorService {
    Intel(IoObject),
    Arm(arm::IOAVService),
}

/// A handle to an attached monitor that allows the use of DDC/CI operations.
#[derive(Debug)]
pub struct Monitor {
    monitor: CGDisplay,
    service: MonitorService,
    i2c_address: u16,
    delay: Delay,
}

impl fmt::Display for Monitor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl Monitor {
    /// Create a new monitor from the specified handle.
    fn new(monitor: CGDisplay, service: MonitorService, i2c_address: u16) -> Self {
        Monitor {
            monitor,
            service,
            i2c_address,
            delay: Default::default(),
        }
    }

    /// Enumerate all connected physical monitors returning [Vec<Monitor>]
    pub fn enumerate() -> Result<Vec<Self>, Error> {
        let monitors = CGDisplay::active_displays()
            .map_err(Error::from)?
            .into_iter()
            .filter_map(|display_id| {
                let display = CGDisplay::new(display_id);
                return if let Some(service) = intel::get_io_framebuffer_port(display) {
                    Some(Self::new(display, MonitorService::Intel(service), I2C_ADDRESS_DDC_CI))
                } else if let Ok((service, i2c_address)) = arm::get_display_av_service(display) {
                    Some(Self::new(display, MonitorService::Arm(service), i2c_address))
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

    fn encode_command<'a>(&self, data: &[u8], packet: &'a mut [u8]) -> &'a [u8] {
        packet[0] = SUB_ADDRESS_DDC_CI;
        packet[1] = 0x80 | data.len() as u8;
        packet[2..2 + data.len()].copy_from_slice(data);
        packet[2 + data.len()] =
            Self::checksum(iter::once((self.i2c_address as u8) << 1).chain(packet[..2 + data.len()].iter().cloned()));
        &packet[..3 + data.len()]
    }

    fn decode_response<'a>(&self, response: &'a mut [u8]) -> Result<&'a mut [u8], crate::error::Error> {
        if response.is_empty() {
            return Ok(response);
        };
        let len = (response[1] & 0x7f) as usize;
        if len + 2 >= response.len() {
            return Err(Error::Ddc(ErrorCode::InvalidLength));
        }
        let checksum = Self::checksum(
            iter::once(((self.i2c_address << 1) | 1) as u8)
                .chain(iter::once(SUB_ADDRESS_DDC_CI))
                .chain(response[1..2 + len].iter().cloned()),
        );
        if response[2 + len] != checksum {
            return Err(Error::Ddc(ErrorCode::InvalidChecksum));
        }
        Ok(&mut response[2..2 + len])
    }
}

impl DdcHost for Monitor {
    type Error = Error;

    fn sleep(&mut self) {
        self.delay.sleep()
    }
}

impl DdcCommandRaw for Monitor {
    fn execute_raw<'a>(
        &mut self,
        data: &[u8],
        out: &'a mut [u8],
        response_delay: Duration,
    ) -> Result<&'a mut [u8], Self::Error> {
        assert!(data.len() <= 36);
        let mut packet = [0u8; 36 + 3];
        let packet = self.encode_command(data, &mut packet);
        let response = match &self.service {
            MonitorService::Intel(service) => intel::execute(service, self.i2c_address, packet, out, response_delay),
            MonitorService::Arm(service) => arm::execute(service, self.i2c_address, packet, out, response_delay),
        }?;
        self.decode_response(response)
    }
}

impl DdcCommandMarker for Monitor {}

impl DdcCommandRawMarker for Monitor {
    fn set_sleep_delay(&mut self, delay: Delay) {
        self.delay = delay;
    }
}
