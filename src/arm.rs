use crate::error::Error;
use crate::error::Error::{DisplayLocationNotFound, ServiceNotFound};
use crate::iokit::IoIterator;
use crate::iokit::{CoreDisplay_DisplayCreateInfoDictionary, IoObject};
use crate::kern_try;
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_foundation_sys::base::{kCFAllocatorDefault, CFAllocatorRef, CFTypeRef, OSStatus};
use core_graphics::display::CGDisplay;
use ddc::I2C_ADDRESS_DDC_CI;
use io_kit_sys::keys::kIOServicePlane;
use io_kit_sys::types::{io_object_t, io_registry_entry_t};
use io_kit_sys::{
    kIORegistryIterateRecursively, IORegistryEntryCreateCFProperty, IORegistryEntryGetName,
    IORegistryEntryGetParentEntry, IORegistryEntryGetPath,
};
use mach::kern_return::KERN_SUCCESS;
use std::ffi::CStr;
use std::os::raw::{c_uint, c_void};

pub type IOAVService = CFTypeRef;

#[link(name = "CoreDisplay", kind = "framework")]
extern "C" {
    // Creates an IOAVService from an existing I/O Kit service
    pub fn IOAVServiceCreateWithService(allocator: CFAllocatorRef, service: io_object_t) -> IOAVService;

    // Reads data over I2C from the specified IOAVService
    pub fn IOAVServiceReadI2C(
        service: IOAVService,
        chip_address: c_uint,
        offset: c_uint,
        output_buffer: *mut c_void,
        output_buffer_size: c_uint,
    ) -> OSStatus;

    // Writes data over I2C to the specified IOAVService
    pub fn IOAVServiceWriteI2C(
        service: IOAVService,
        chip_address: c_uint,
        data_address: c_uint,
        input_buffer: *const c_void,
        input_buffer_size: c_uint,
    ) -> OSStatus;
}

/// Returns an AVService and its DDC I2C address for a given display
pub fn get_display_av_service(display: CGDisplay) -> Result<(IOAVService, u16), Error> {
    if display.is_builtin() {
        return Err(ServiceNotFound);
    }
    let display_infos: CFDictionary<CFString, CFType> =
        unsafe { CFDictionary::wrap_under_create_rule(CoreDisplay_DisplayCreateInfoDictionary(display.id)) };
    let location = display_infos
        .find(CFString::from_static_string("IODisplayLocation"))
        .ok_or(DisplayLocationNotFound)?
        .downcast::<CFString>()
        .ok_or(DisplayLocationNotFound)?
        .to_string();
    let external_location = CFString::from_static_string("External").into_CFType();

    let mut iter = IoIterator::root()?;
    while let Some(service) = iter.next() {
        if let Ok(registry_location) = get_service_registry_entry_path(service.as_raw()) {
            if registry_location == location {
                while let Some(service) = iter.next() {
                    if get_service_registry_entry_name(service.as_raw())? == "DCPAVServiceProxy" {
                        let av_service = unsafe { IOAVServiceCreateWithService(kCFAllocatorDefault, service.as_raw()) };
                        let loc_ref = unsafe {
                            IORegistryEntryCreateCFProperty(
                                service.as_raw(),
                                CFString::from_static_string("Location").as_concrete_TypeRef(),
                                kCFAllocatorDefault,
                                kIORegistryIterateRecursively,
                            )
                        };
                        if !loc_ref.is_null() {
                            let loc_ref = unsafe { CFType::wrap_under_create_rule(loc_ref) };
                            if !av_service.is_null() && (loc_ref == external_location) {
                                return Ok((av_service, i2c_address(service)));
                            }
                        }
                    }
                }
            }
        }
    }
    Err(ServiceNotFound)
}

const I2C_ADDRESS_DDC_CI_MDCP29XX: u16 = 0xB7;

/// Returns the I2C chip address for a given service
fn i2c_address(service: IoObject) -> u16 {
    // M1 Macs use a non-standard chip address on their builtin HDMI ports: they are behind a
    // MDCP29xx DisplayPort to HDMI bridge chip, and it needs a different I2C slave address:
    // not a standard 0x37 but 0xB7.
    let mut parent: io_registry_entry_t = 0;
    unsafe {
        if IORegistryEntryGetParentEntry(service.as_raw(), kIOServicePlane, &mut parent) != KERN_SUCCESS {
            return I2C_ADDRESS_DDC_CI;
        }
    }
    let class_ref = unsafe {
        IORegistryEntryCreateCFProperty(
            parent,
            CFString::from_static_string("EPICProviderClass").as_concrete_TypeRef(),
            kCFAllocatorDefault,
            kIORegistryIterateRecursively,
        )
    };
    if class_ref.is_null() {
        return I2C_ADDRESS_DDC_CI;
    }
    let mcdp29xx = CFString::from_static_string("AppleDCPMCDP29XX").into_CFType();
    let class_ref = unsafe { CFType::wrap_under_create_rule(class_ref) };
    if class_ref == mcdp29xx {
        I2C_ADDRESS_DDC_CI_MDCP29XX
    } else {
        I2C_ADDRESS_DDC_CI
    }
}

fn get_service_registry_entry_path(entry: io_registry_entry_t) -> Result<String, Error> {
    let mut path_buffer = [0_i8; 1024];
    unsafe {
        kern_try!(IORegistryEntryGetPath(entry, kIOServicePlane, path_buffer.as_mut_ptr()));
        Ok(CStr::from_ptr(path_buffer.as_ptr()).to_string_lossy().into_owned())
    }
}

fn get_service_registry_entry_name(entry: io_registry_entry_t) -> Result<String, Error> {
    let mut name = [0; 128];
    unsafe {
        kern_try!(IORegistryEntryGetName(entry, name.as_mut_ptr()));
        Ok(CStr::from_ptr(name.as_ptr()).to_string_lossy().into_owned())
    }
}
