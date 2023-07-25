use core_foundation::base::{kCFAllocatorDefault, CFType, TCFType};
use core_foundation::dictionary::{CFDictionary, CFMutableDictionary, CFMutableDictionaryRef};
use core_foundation::string::CFString;
use io_kit_sys::types::{io_iterator_t, io_object_t};
use io_kit_sys::{
    kIOMasterPortDefault, IOIteratorNext, IOObjectRelease, IORegistryEntryCreateCFProperties,
    IOServiceGetMatchingServices, IOServiceMatching, IOServiceNameMatching,
};
use std::ops::{Deref, DerefMut};

#[derive(Debug)]
pub struct IoObject(io_object_t);

impl IoObject {
    /// Returns typed dictionary with this object properties.
    pub fn properties(&self) -> Result<CFDictionary<CFString, CFType>, std::io::Error> {
        unsafe {
            let mut props = std::ptr::null_mut();
            kern_try!(IORegistryEntryCreateCFProperties(
                self.0,
                &mut props,
                kCFAllocatorDefault as _,
                0
            ));
            Ok(CFMutableDictionary::wrap_under_create_rule(props as _).to_immutable())
        }
    }
}

impl From<io_object_t> for IoObject {
    fn from(object: io_object_t) -> Self {
        Self(object)
    }
}

impl From<&IoObject> for io_object_t {
    fn from(val: &IoObject) -> io_object_t {
        val.0
    }
}

impl Drop for IoObject {
    fn drop(&mut self) {
        unsafe { IOObjectRelease(self.0) };
    }
}

#[derive(Debug)]
pub struct IoIterator(io_iterator_t);

impl IoIterator {
    pub fn for_service_names(name: &str) -> Option<Self> {
        let c_name = std::ffi::CString::new(name).ok()?;
        let dict = unsafe { IOServiceNameMatching(c_name.as_ptr()) };
        Self::matching_services(dict as _).ok()
    }

    pub fn for_services(name: &str) -> Option<Self> {
        let c_name = std::ffi::CString::new(name).ok()?;
        let dict = unsafe { IOServiceMatching(c_name.as_ptr()) };
        Self::matching_services(dict as _).ok()
    }

    fn matching_services(dict: CFMutableDictionaryRef) -> Result<Self, std::io::Error> {
        let mut iter: io_iterator_t = 0;
        unsafe {
            kern_try!(IOServiceGetMatchingServices(kIOMasterPortDefault, dict as _, &mut iter));
        }
        Ok(Self(iter))
    }
}

impl Deref for IoIterator {
    type Target = io_iterator_t;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for IoIterator {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Iterator for IoIterator {
    type Item = IoObject;

    fn next(&mut self) -> Option<Self::Item> {
        match unsafe { IOIteratorNext(self.0) } {
            0 => None,
            io_object => Some(IoObject(io_object)),
        }
    }
}

impl Drop for IoIterator {
    fn drop(&mut self) {
        unsafe { IOObjectRelease(self.0) };
    }
}
