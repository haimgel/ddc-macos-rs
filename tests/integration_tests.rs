extern crate ddc_macos;
use ddc::Ddc;

#[test]
#[ignore]
/// Test getting current monitor inputs, this would fail on CI.
fn test_get_vcp_feature() {
    let mut monitors = ddc_macos::Monitor::enumerate().unwrap();
    assert_ne!(monitors.len(), 0);

    for monitor in monitors.iter_mut() {
        let input = monitor.get_vcp_feature(0x60);
        assert!(input.is_ok());
    }
}
