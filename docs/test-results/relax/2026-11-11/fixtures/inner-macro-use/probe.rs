mod macros;
mod child;
#[test]
fn inherited_inner_attribute_macro() { assert_eq!(child::value(), 17); }
