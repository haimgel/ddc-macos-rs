# ![allow(non_upper_case_globals,unused)]

/// Selective translation of IOKit/graphics/IOGraphicsLib.h

use IOKit_sys::{IOOptionBits, io_service_t};
use core_foundation::dictionary::CFDictionaryRef;

pub const kIODisplayMatchingInfo: IOOptionBits      = 0x00000100;
pub const kIODisplayOnlyPreferredName: IOOptionBits = 0x00000200;
pub const kIODisplayNoProductName: IOOptionBits     = 0x00000400;

extern "C" {
    #[link(name = "IOKit", kind = "framework")]
    pub fn IODisplayCreateInfoDictionary(framebuffer: io_service_t, options: IOOptionBits) -> CFDictionaryRef;
}
