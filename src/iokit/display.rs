#![allow(non_upper_case_globals, unused)]

/// Selective translation of IOKit/graphics/IOGraphicsLib.h
use core_foundation::dictionary::CFDictionaryRef;
use core_graphics::display::CGDirectDisplayID;
use io_kit_sys::types::{io_service_t, IOOptionBits};
use mach2::port::{mach_port_t, MACH_PORT_NULL};

pub const kIODisplayMatchingInfo: IOOptionBits = 0x00000100;
pub const kIODisplayOnlyPreferredName: IOOptionBits = 0x00000200;
pub const kIODisplayNoProductName: IOOptionBits = 0x00000400;

// IOKitLib.h: "When specifying a main port to IOKit functions, the NULL argument
// indicates 'use the default'." kIOMainPortDefault is a synonym for that NULL, but
// it is only exported since macOS 12, so it is defined here instead of imported.
pub const kIOMainPortDefault: mach_port_t = MACH_PORT_NULL;

extern "C" {
    #[link(name = "IOKit", kind = "framework")]
    pub fn IODisplayCreateInfoDictionary(framebuffer: io_service_t, options: IOOptionBits) -> CFDictionaryRef;

    #[link(name = "CoreDisplay", kind = "framework")]
    // Creates a display info dictionary for a specified display ID
    pub fn CoreDisplay_DisplayCreateInfoDictionary(display_id: CGDirectDisplayID) -> CFDictionaryRef;
}
