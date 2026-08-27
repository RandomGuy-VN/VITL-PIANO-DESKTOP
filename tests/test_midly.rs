#[test]
fn test_timing() {
    println!("fps as int: {}", midly::Fps::Fps24.as_int());
    println!("fps as f32: {}", midly::Fps::Fps24.as_f32());
}
