use evdev::{uinput::VirtualDevice, KeyCode, AttributeSet};
#[test]
fn test_ui() {
    let mut keys = AttributeSet::<KeyCode>::new();
    keys.insert(KeyCode::KEY_A);
    match VirtualDevice::builder() {
        Ok(builder) => match builder.name("VITL").with_keys(&keys) {
            Ok(b) => match b.build() {
                Ok(_) => println!("Uinput built!"),
                Err(e) => println!("Uinput build error: {}", e),
            },
            Err(e) => println!("Uinput keys error: {}", e),
        },
        Err(e) => println!("Uinput new error: {}", e),
    }
}

