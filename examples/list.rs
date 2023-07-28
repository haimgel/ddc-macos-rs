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

            /*
            match edid::parse(data: &[u8]) {
                nom::IResult::Done(remaining, parsed) => {
                    assert_eq!(remaining.len(), 0);
                    assert_eq!(&parsed, expected);
                },
                nom::IResult::Error(err) => {
                    panic!(format!("{}", err));
                },
                nom::IResult::Incomplete(_) => {
                    panic!("Incomplete");
                },
            }
             */
        }
    }
}