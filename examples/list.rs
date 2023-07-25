extern crate ddc;
extern crate ddc_macos;

use ddc::Ddc;
use ddc_macos::Monitor;

fn main() {
    let monitors = Monitor::enumerate()
        .expect("Could not enumerate external monitors");

    if monitors.is_empty() {
        println!("No external monitors found");
    } else {
        for mut monitor in monitors {
            let input = monitor.get_vcp_feature(0x60)
                .expect("Could not get feature 0x60");
            println!("Current input: {:04x}", input.value());
            println!("Monitor description: {}", monitor.description());
        }
    }
}