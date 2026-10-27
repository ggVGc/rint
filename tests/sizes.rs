use rint::interp::R;
use rint::value::Value;

#[test]
fn value_is_16_bytes() {
    println!("Value={} R<Value>={} ExprKind={}", std::mem::size_of::<Value>(), std::mem::size_of::<R<Value>>(), std::mem::size_of::<rint::ast::ExprKind>());
    assert_eq!(std::mem::size_of::<Value>(), 16);
    assert_eq!(std::mem::size_of::<R<Value>>(), 16);
}
