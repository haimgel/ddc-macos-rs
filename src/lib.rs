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

mod iokit;
mod monitor;

pub use monitor::*;
