macro_rules! outer_value { () => { inner_value!() }; }
pub fn value() -> u32 { outer_value!() }
