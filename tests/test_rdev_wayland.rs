use rdev::{simulate, EventType, Key};
#[test]
fn test_sim() {
    match simulate(&EventType::KeyPress(Key::KeyA)) {
        Ok(_) => println!("Simulate success!"),
        Err(e) => println!("Simulate error: {:?}", e),
    }
}
