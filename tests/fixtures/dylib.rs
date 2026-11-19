#[unsafe(no_mangle)]
pub extern "C" fn zsys_fixture_add_one(value: u32) -> u32 {
    value.wrapping_add(1)
}
