#![allow(non_camel_case_types,non_upper_case_globals,non_snake_case,unused)]

/// Translation of IOKit/i2c/IOI2CInterface.h
extern crate io_kit_sys;
extern crate mach;

use crate::iokit::wrappers::IoObject;
use io_kit_sys::ret::IOReturn;
use io_kit_sys::types::{io_service_t, IOItemCount, IOOptionBits};
use mach::vm_types::{mach_vm_address_t, mach_vm_size_t, vm_address_t};
use std::os::raw::c_uint;

/// IOI2CRequest.sendTransactionType, IOI2CRequest.replyTransactionType
pub const kIOI2CNoTransactionType: c_uint = 0;
pub const kIOI2CSimpleTransactionType: c_uint = 1;
pub const kIOI2CDDCciReplyTransactionType: c_uint = 2;
pub const kIOI2CCombinedTransactionType: c_uint = 3;
pub const kIOI2CDisplayPortNativeTransactionType: c_uint = 4;

pub type IOI2CRequestCompletion = ::std::option::Option<unsafe extern "C" fn(request: *mut IOI2CRequest)>;

#[repr(C, packed(4))]
#[derive(Debug, Copy, Clone)]
///  A structure defining an I2C bus transaction
pub struct IOI2CRequest {
    pub sendTransactionType: IOOptionBits,
    pub replyTransactionType: IOOptionBits,
    pub sendAddress: u32,
    pub replyAddress: u32,
    pub sendSubAddress: u8,
    pub replySubAddress: u8,
    pub __reservedA: [u8; 2usize],
    pub minReplyDelay: u64,
    pub result: IOReturn,
    pub commFlags: IOOptionBits,
    pub __padA: u32,
    pub sendBytes: u32,
    pub __reservedB: [u32; 2usize],
    pub __padB: u32,
    pub replyBytes: u32,
    pub completion: IOI2CRequestCompletion,
    pub sendBuffer: vm_address_t,
    pub replyBuffer: vm_address_t,
    pub __reservedC: [u32; 10usize],
}

/// struct IOI2CConnect is opaque
#[repr(C)]
pub struct IOI2CConnect {
    _opaque: [u8; 0],
}
type IOI2CConnectRef = *mut IOI2CConnect;

extern "C" {
    #[link(name = "IOKit", kind = "framework")]

    /// Returns a count of I2C interfaces available associated with an IOFramebuffer instance
    pub fn IOFBGetI2CInterfaceCount(framebuffer: io_service_t, count: *mut IOItemCount) -> IOReturn;

    /// Returns an instance of an I2C bus interface, associated with an IOFramebuffer instance / bus index pair
    pub fn IOFBCopyI2CInterfaceForBus(
        framebuffer: io_service_t,
        bus: IOOptionBits,
        interface: *mut io_service_t,
    ) -> IOReturn;

    /// Opens an instance of an I2C bus interface, allowing I2C requests to be made
    pub fn IOI2CInterfaceOpen(
        interface: io_service_t,
        options: IOOptionBits,
        connect: *mut IOI2CConnectRef,
    ) -> IOReturn;

    /// Closes an IOI2CConnectRef
    pub fn IOI2CInterfaceClose(connect: IOI2CConnectRef, options: IOOptionBits) -> IOReturn;

    /// Carries out the I2C transaction specified by an IOI2CRequest structure
    pub fn IOI2CSendRequest(connect: IOI2CConnectRef, options: IOOptionBits, request: *mut IOI2CRequest) -> IOReturn;
}

pub(crate) struct IoI2CInterfaceConnection(IOI2CConnectRef);

impl IoI2CInterfaceConnection {
    pub fn new(interface: &IoObject) -> Result<Self, std::io::Error> {
        let mut handle = std::ptr::null_mut();
        unsafe {
            kern_try!(IOI2CInterfaceOpen(interface.into(), 0, &mut handle));
        }
        Ok(Self(handle))
    }

    pub fn send_request(&self, request: *mut IOI2CRequest) -> Result<(), std::io::Error> {
        unsafe {
            kern_try!(IOI2CSendRequest(self.0, 0, request));
            kern_try!((*request).result);
        }
        Ok(())
    }
}

impl Drop for IoI2CInterfaceConnection {
    fn drop(&mut self) {
        unsafe {
            IOI2CInterfaceClose(self.0, 0);
        }
    }
}
