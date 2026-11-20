mod macros {
    #![macro_use]
    macro_rules! inner_value { () => { 17u32 }; }
}
mod child {
    macro_rules! outer_value { () => { inner_value!() }; }
    pub fn value() -> u32 { outer_value!() }
}
#[test]
fn inherited_inline_attribute_macro() { assert_eq!(child::value(), 17); }
