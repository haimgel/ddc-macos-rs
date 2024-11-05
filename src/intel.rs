use crate::error::{verify_io, Error};
use crate::iokit::{kIODisplayOnlyPreferredName, IODisplayCreateInfoDictionary};
use crate::iokit::{
    kIOI2CDDCciReplyTransactionType, kIOI2CSimpleTransactionType, IOFBCopyI2CInterfaceForBus, IOFBGetI2CInterfaceCount,
    IOI2CRequest, IoI2CInterfaceConnection,
};
use crate::iokit::{IoIterator, IoObject};
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::base::kCFAllocatorDefault;
use core_graphics::display::CGDisplay;
use io_kit_sys::ret::kIOReturnSuccess;
use io_kit_sys::types::{io_service_t, IOItemCount};
use io_kit_sys::IORegistryEntryCreateCFProperties;
use mach::kern_return::KERN_FAILURE;

pub fn display_info_dict(frame_buffer: &IoObject) -> Option<CFDictionary<CFString, CFType>> {
    unsafe {
        let info = IODisplayCreateInfoDictionary(frame_buffer.into(), kIODisplayOnlyPreferredName).as_ref()?;
        Some(CFDictionary::<CFString, CFType>::wrap_under_create_rule(info))
    }
}

/// Get supported I2C / DDC transaction types
/// DDCciReply is what we want, but Simple will also work
pub unsafe fn get_supported_transaction_type() -> Option<u32> {
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

/// Finds if a framebuffer that matches display
fn framebuffer_port_matches_display(port: &IoObject, display: CGDisplay) -> Option<()> {
    let mut bus_count: IOItemCount = 0;
    unsafe {
        IOFBGetI2CInterfaceCount(port.into(), &mut bus_count);
    }
    if bus_count == 0 {
        return None;
    };

    let info = display_info_dict(port)?;

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

/// Gets the framebuffer port for a display
pub fn get_io_framebuffer_port(display: CGDisplay) -> Option<IoObject> {
    if display.is_builtin() {
        return None;
    }
    IoIterator::for_services("IOFramebuffer")?
        .find(|framebuffer| framebuffer_port_matches_display(framebuffer, display).is_some())
}

/// send an I2C request to a display
pub(crate) unsafe fn send_request(
    service: &IoObject,
    request: &mut IOI2CRequest,
    // post_request_delay: u32,
) -> Result<(), Error> {
    let mut bus_count = 0;
    let mut result: Result<(), Error> = Err(Error::Io(KERN_FAILURE));
    verify_io(IOFBGetI2CInterfaceCount(service.into(), &mut bus_count))?;
    for bus in 0..bus_count {
        let mut interface: io_service_t = 0;
        if IOFBCopyI2CInterfaceForBus(service.into(), bus, &mut interface) == kIOReturnSuccess {
            let interface = IoObject::from(interface);
            result = IoI2CInterfaceConnection::new(&interface)
                .and_then(|connection| connection.send_request(request))
                .map_err(From::from);
            if result.is_ok() {
                break;
            }
        }
    }
    // std::thread::sleep(std::time::Duration::from_millis(post_request_delay as u64));
    result
}
