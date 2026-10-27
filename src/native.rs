//! Checked conversion and dispatch for compiled host functions.
use crate::value::{Value, new_list};
use std::{cell::RefCell, rc::Rc};

pub type NativeCallback = dyn Fn(&[Value]) -> Result<Value, String>;

#[derive(Clone)]
pub struct NativeBinding {
    pub path: String,
    pub callback: Rc<NativeCallback>,
}

/// Convert an interpreter value to an owned native argument.
pub trait FromValue: Sized {
    fn from_value(value: &Value) -> Result<Self, String>;
}

/// Convert a native return value to an interpreter value.
pub trait IntoValue {
    fn into_value(self) -> Result<Value, String>;
}

fn mismatch(expected: &str, value: &Value) -> String {
    format!("expected {expected}, got {}", value.type_name())
}

macro_rules! integers {
    ($($ty:ty),*) => {$(
        impl FromValue for $ty {
            fn from_value(value: &Value) -> Result<Self, String> {
                match value {
                    Value::Int(n) => Self::try_from(*n).map_err(|_| format!("integer {n} is out of range for {}", stringify!($ty))),
                    _ => Err(mismatch(stringify!($ty), value)),
                }
            }
        }
        impl IntoValue for $ty {
            fn into_value(self) -> Result<Value, String> {
                i64::try_from(self).map(Value::Int).map_err(|_| "native integer is out of range for rint's i64".into())
            }
        }
    )*};
}
integers!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);

macro_rules! scalar {
    ($ty:ty, $variant:ident) => {
        impl FromValue for $ty {
            fn from_value(value: &Value) -> Result<Self, String> {
                match value {
                    Value::$variant(v) => Ok(*v),
                    _ => Err(mismatch(stringify!($ty), value)),
                }
            }
        }
        impl IntoValue for $ty {
            fn into_value(self) -> Result<Value, String> {
                Ok(Value::$variant(self))
            }
        }
    };
}
scalar!(bool, Bool);
scalar!(char, Char);
scalar!(f64, Float);

impl FromValue for f32 {
    fn from_value(value: &Value) -> Result<Self, String> {
        let n = f64::from_value(value)?;
        if n.is_finite() && !(n as f32).is_finite() {
            return Err("float is out of range for f32".into());
        }
        Ok(n as f32)
    }
}
impl IntoValue for f32 {
    fn into_value(self) -> Result<Value, String> {
        Ok(Value::Float(self as f64))
    }
}
impl FromValue for () {
    fn from_value(value: &Value) -> Result<Self, String> {
        if matches!(value, Value::Unit) {
            Ok(())
        } else {
            Err(mismatch("()", value))
        }
    }
}
impl IntoValue for () {
    fn into_value(self) -> Result<Value, String> {
        Ok(Value::Unit)
    }
}
impl FromValue for String {
    fn from_value(value: &Value) -> Result<Self, String> {
        match value {
            Value::Str(s) => Ok(s.as_ref().clone()),
            Value::String(s) => Ok(s.borrow().clone()),
            _ => Err(mismatch("string", value)),
        }
    }
}
impl IntoValue for String {
    fn into_value(self) -> Result<Value, String> {
        Ok(Value::String(Rc::new(RefCell::new(self))))
    }
}
impl IntoValue for &str {
    fn into_value(self) -> Result<Value, String> {
        self.to_owned().into_value()
    }
}
impl<T: FromValue> FromValue for Vec<T> {
    fn from_value(value: &Value) -> Result<Self, String> {
        match value {
            Value::Vec(v) | Value::Array(v) => v.borrow().iter().map(T::from_value).collect(),
            _ => Err(mismatch("vector", value)),
        }
    }
}
impl<T: IntoValue> IntoValue for Vec<T> {
    fn into_value(self) -> Result<Value, String> {
        Ok(Value::Vec(new_list(
            self.into_iter()
                .map(IntoValue::into_value)
                .collect::<Result<_, _>>()?,
        )))
    }
}
impl<T: IntoValue, E: IntoValue> IntoValue for Result<T, E> {
    fn into_value(self) -> Result<Value, String> {
        let (id, value) = match self {
            Ok(v) => (crate::program::V_OK, v.into_value()?),
            Err(e) => (crate::program::V_ERR, e.into_value()?),
        };
        Ok(Value::Enum(id, Some(new_list(vec![value]))))
    }
}

/// Implemented for functions taking up to six owned arguments.
pub trait NativeFunction<Args>: 'static {
    fn call(&self, args: &[Value]) -> Result<Value, String>;
}
macro_rules! functions {
    ($count:expr; $($T:ident:$i:tt),*) => {
        impl<F, R, $($T,)*> NativeFunction<($($T,)*)> for F
        where F: Fn($($T),*) -> R + 'static, R: IntoValue, $($T: FromValue,)*
        {
            fn call(&self, args: &[Value]) -> Result<Value, String> {
                if args.len() != $count {
                    return Err(format!("expected {} arguments, got {}", $count, args.len()));
                }
                self($($T::from_value(&args[$i]).map_err(|e| format!("argument {}: {e}", $i + 1))?),*).into_value()
            }
        }
    };
}
functions!(0;);
functions!(1; A:0);
functions!(2; A:0, B:1);
functions!(3; A:0, B:1, C:2);
functions!(4; A:0, B:1, C:2, D:3);
functions!(5; A:0, B:1, C:2, D:3, E:4);
functions!(6; A:0, B:1, C:2, D:3, E:4, G:5);

/// Adapter marker for a function accepting a string slice.
pub struct BorrowedStr;
impl<F, R> NativeFunction<BorrowedStr> for F
where
    F: Fn(&str) -> R + 'static,
    R: IntoValue,
{
    fn call(&self, args: &[Value]) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!("expected 1 arguments, got {}", args.len()));
        }
        let text = String::from_value(&args[0]).map_err(|e| format!("argument 1: {e}"))?;
        self(&text).into_value()
    }
}
