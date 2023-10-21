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
            println!("Monitor");
            println!("\tDescription: {}", monitor.description());
            if let Some(desc) = monitor.product_name() {
                println!("\tProduct Name: {}", desc);
            }
            if let Some(number) = monitor.serial_number() {
                println!("\tSerial Number: {}", number);
            }
            if let Ok(input) = monitor.get_vcp_feature(0x60) {
                println!("\tCurrent input: {:04x}", input.value());
            }

            if let Some(data) = monitor.edid() {
                let mut cursor = std::io::Cursor::new(&data);
                let mut reader = edid_rs::Reader::new(&mut cursor);
                match edid_rs::EDID::parse(&mut reader) {
                    Ok(edid) => println!("\tEDID Info: {:?}", edid),
                    _ => println!("\tCould not parse provided EDID information"),
                }
            }
        }
    }
}