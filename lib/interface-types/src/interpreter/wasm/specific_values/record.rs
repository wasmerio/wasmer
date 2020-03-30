//! Specific operations on records.

#![allow(missing_docs)]

use crate::interpreter::wasm::values::InterfaceValue;
use serde::{
    de::{self, DeserializeSeed, SeqAccess, Visitor},
    Deserialize,
};
use std::{
    fmt::{self, Display},
    iter::Peekable,
    slice::Iter,
};

/// Iterates over a vector of `InterfaceValues` but flatten all the
/// values for Serde. It means that the ideal representation for Serde
/// regarding our implementation is to get all values flatten. So
/// `I32(1), Record([I32(2), I32(3)]), I32(4)` must be iterated like
/// `I32(1), I32(2), I32(3), I32(4)`.
struct InterfaceValueIterator<'a> {
    iterators: Vec<Iter<'a, InterfaceValue>>,
}

impl<'a> InterfaceValueIterator<'a> {
    fn new(values: &'a [InterfaceValue]) -> Self {
        Self {
            iterators: vec![values.iter()],
        }
    }
}

impl<'a> Iterator for InterfaceValueIterator<'a> {
    type Item = &'a InterfaceValue;

    fn next(&mut self) -> Option<Self::Item> {
        if self.iterators.is_empty() {
            return None;
        }

        let index = self.iterators.len() - 1;

        match self.iterators[index].next() {
            // End of the current iterator, go back to the previous
            // one.
            None => {
                self.iterators.pop();
                self.next()
            }

            // Recursively iterate over the record.
            Some(InterfaceValue::Record(values)) => {
                self.iterators.push(values.iter());
                self.next()
            }

            // A regular item.
            e @ Some(_) => e,
        }
    }
}

struct Deserializer<'de> {
    iterator: Peekable<InterfaceValueIterator<'de>>,
}

impl<'de> Deserializer<'de> {
    pub fn from_values(input: &'de [InterfaceValue]) -> Deserializer<'de> {
        Deserializer {
            iterator: InterfaceValueIterator::new(input).peekable(),
        }
    }
}

macro_rules! next {
    ($method_name:ident, $variant:ident, $type:ty) => {
        fn $method_name(&mut self) -> Result<$type, Error> {
            match self.iterator.peek() {
                Some(InterfaceValue::$variant(v)) => {
                    self.iterator.next();

                    Ok(*v)
                }

                Some(_) => Err(Error::TypeMismatch),

                None => Err(Error::InputEmpty),
            }
        }
    }
}

impl<'de> Deserializer<'de> {
    next!(next_s8, S8, i8);
    next!(next_s16, S16, i16);
    next!(next_s32, S32, i32);
    next!(next_s64, S64, i64);
    next!(next_u8, U8, u8);
    next!(next_u16, U16, u16);
    next!(next_u32, U32, u32);
    next!(next_u64, U64, u64);
    next!(next_f32, F32, f32);
    next!(next_f64, F64, f64);

    fn next_string(&mut self) -> Result<&'de str, Error> {
        match self.iterator.peek() {
            Some(InterfaceValue::String(v)) => {
                self.iterator.next();

                Ok(v)
            }

            Some(_) => Err(Error::TypeMismatch),

            None => Err(Error::InputEmpty),
        }
    }

    next!(next_i32, I32, i32);
    next!(next_i64, I64, i64);
}

pub fn from_values<'a, T>(s: &'a [InterfaceValue]) -> Result<T, Error>
where
    T: Deserialize<'a>,
{
    let mut deserializer = Deserializer::from_values(s);
    let t = T::deserialize(&mut deserializer)?;

    if deserializer.iterator.peek().is_none() {
        Ok(t)
    } else {
        Err(Error::InputNotEmpty)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    InputNotEmpty,
    InputEmpty,
    TypeMismatch,
    Message(String),
}

impl de::Error for Error {
    fn custom<T: Display>(msg: T) -> Self {
        Error::Message(msg.to_string())
    }
}

impl Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(std::error::Error::description(self))
    }
}

impl std::error::Error for Error {
    fn description(&self) -> &str {
        match *self {
            Error::InputNotEmpty => "unexpected input remaining",
            Error::Message(ref msg) => msg,
            Error::InputEmpty => "unexpected end of input",
            Error::TypeMismatch => "type mismatch detected",
        }
    }
}

impl<'de, 'a> de::Deserializer<'de> for &'a mut Deserializer<'de> {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.iterator.peek() {
            Some(InterfaceValue::S8(_)) => self.deserialize_i8(visitor),
            Some(InterfaceValue::S16(_)) => self.deserialize_i16(visitor),
            Some(InterfaceValue::S32(_)) => self.deserialize_i32(visitor),
            Some(InterfaceValue::S64(_)) => self.deserialize_i64(visitor),
            Some(InterfaceValue::U8(_)) => self.deserialize_u8(visitor),
            Some(InterfaceValue::U16(_)) => self.deserialize_u16(visitor),
            Some(InterfaceValue::U32(_)) => self.deserialize_u32(visitor),
            Some(InterfaceValue::U64(_)) => self.deserialize_u64(visitor),
            Some(InterfaceValue::F32(_)) => self.deserialize_f32(visitor),
            Some(InterfaceValue::F64(_)) => self.deserialize_f64(visitor),
            Some(InterfaceValue::String(_)) => self.deserialize_string(visitor),
            Some(InterfaceValue::I32(_)) => self.deserialize_i32(visitor),
            Some(InterfaceValue::I64(_)) => self.deserialize_i64(visitor),
            Some(InterfaceValue::Record(_)) => unreachable!("Records should have been flatten."), // already flatten
            None => Err(Error::InputEmpty),
        }
    }

    fn deserialize_bool<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        unimplemented!("`bool` is not supported by WIT for the moment.")
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_i8(self.next_s8()?)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_i16(self.next_s16()?)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        // Both `InterfaceValue::S32` and `InterfaceValue::I32`
        // represent `i32`.
        visitor.visit_i32(self.next_s32().or(self.next_i32())?)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        // Both `InterfaceValue::S64` and `InterfaceValue::I64`
        // represent `i64`.
        visitor.visit_i64(self.next_s64().or(self.next_i64())?)
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_u8(self.next_u8()?)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_u16(self.next_u16()?)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_u32(self.next_u32()?)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_u64(self.next_u64()?)
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_f32(self.next_f32()?)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_f64(self.next_f64()?)
    }

    fn deserialize_char<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        todo!("`char` is not supported by WIT for the moment.")
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_borrowed_str(self.next_string()?)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        todo!("`bytes` is not supported by WIT for the moment.")
    }

    fn deserialize_byte_buf<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        todo!("`bytes` buffer is not supported by WIT for the moment.")
    }

    fn deserialize_option<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        todo!("`option` is not supported by WIT for the moment.")
    }

    fn deserialize_unit<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        todo!("`unit` is not supported by WIT for the moment.")
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        todo!("`unit_struct` is not supported by WIT for the moment.")
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_seq<V>(mut self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_seq(Sequence::new(&mut self))
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        todo!("`map` is not supported by WIT for the moment.")
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        todo!("`enum` is not supported by WIT for the moment.")
    }

    fn deserialize_identifier<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        todo!("`identifier` is not supported by WIT for the moment.");
    }

    fn deserialize_ignored_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        todo!("`ignored_any` is not implemented for the moment.")
    }
}

struct Sequence<'a, 'de>
where
    'de: 'a,
{
    de: &'a mut Deserializer<'de>,
}

impl<'a, 'de> Sequence<'a, 'de> {
    fn new(de: &'a mut Deserializer<'de>) -> Self {
        Sequence { de }
    }
}

impl<'de, 'a> SeqAccess<'de> for Sequence<'a, 'de> {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        if self.de.iterator.peek().is_none() {
            return Ok(None);
        }

        seed.deserialize(&mut *self.de).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::TryInto;

    macro_rules! try_into {
        ($ty:ty) => {
            impl TryInto<$ty> for Vec<InterfaceValue> {
                type Error = Error;

                fn try_into(self) -> Result<$ty, Self::Error> {
                    from_values(&self)
                }
            }

            impl TryInto<$ty> for &Vec<InterfaceValue> {
                type Error = Error;

                fn try_into(self) -> Result<$ty, Self::Error> {
                    from_values(self)
                }
            }
        };
    }

    #[test]
    fn test_deserialize_basic() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct S {
            x: i32,
            y: i64,
        }

        try_into!(S);

        let input: S = vec![InterfaceValue::I32(1), InterfaceValue::I64(2)]
            .try_into()
            .unwrap();
        let output = S { x: 1, y: 2 };

        assert_eq!(input, output);
    }

    #[test]
    fn test_deserialize_compound() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct Point {
            x: i32,
            y: i32,
        }

        #[derive(Deserialize, Debug, PartialEq)]
        struct Compound {
            points: (Point, Point),
            more_points: Vec<Point>,
        }

        try_into!(Compound);

        let input: Compound = vec![
            InterfaceValue::I32(1),
            InterfaceValue::I32(2),
            InterfaceValue::I32(3),
            InterfaceValue::I32(4),
            InterfaceValue::I32(5),
            InterfaceValue::I32(6),
            InterfaceValue::I32(7),
            InterfaceValue::I32(8),
            InterfaceValue::I32(9),
            InterfaceValue::I32(10),
        ]
        .try_into()
        .unwrap();
        let output = Compound {
            points: (Point { x: 1, y: 2 }, Point { x: 3, y: 4 }),
            more_points: vec![
                Point { x: 5, y: 6 },
                Point { x: 7, y: 8 },
                Point { x: 9, y: 10 },
            ],
        };

        assert_eq!(input, output);
    }

    #[test]
    fn test_deserialize_newtype_struct() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct S(i32);

        try_into!(S);

        let input: S = vec![InterfaceValue::I32(1)].try_into().unwrap();
        let output = S(1);

        assert_eq!(input, output);
    }

    #[test]
    fn test_deserialize_tuple() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct S {
            x: (i32, i64),
        };

        try_into!(S);

        let input: S = vec![InterfaceValue::I32(1), InterfaceValue::I64(2)]
            .try_into()
            .unwrap();
        let output = S { x: (1, 2) };

        assert_eq!(input, output);
    }

    #[test]
    fn test_deserialize_tuple_struct() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct S(i32, i64);

        try_into!(S);

        let input: S = vec![InterfaceValue::I32(1), InterfaceValue::I64(2)]
            .try_into()
            .unwrap();
        let output = S(1, 2);

        assert_eq!(input, output);
    }

    macro_rules! value {
        ($test_name:ident, $variant:ident, $ty:ident, $value:expr) => {
            #[test]
            #[allow(non_snake_case)]
            fn $test_name() {
                #[derive(Deserialize, Debug, PartialEq)]
                struct S {
                    x: $ty,
                };

                try_into!(S);

                let input: S = vec![InterfaceValue::$variant($value)].try_into().unwrap();
                let output = S { x: $value };

                assert_eq!(input, output);
            }
        };
    }

    value!(test_deserialize_value__s8, S8, i8, 42);
    value!(test_deserialize_value__s16, S16, i16, 42);
    value!(test_deserialize_value__s32, S32, i32, 42);
    value!(test_deserialize_value__s64, S64, i64, 42);
    value!(test_deserialize_value__u8, U8, u8, 42);
    value!(test_deserialize_value__u16, U16, u16, 42);
    value!(test_deserialize_value__u32, U32, u32, 42);
    value!(test_deserialize_value__u64, U64, u64, 42);
    value!(test_deserialize_value__f32, F32, f32, 42.);
    value!(test_deserialize_value__f64, F32, f32, 42.);
    value!(
        test_deserialize_value__string,
        String,
        String,
        "foo".to_string()
    );

    #[test]
    #[allow(non_snake_case)]
    fn test_deserialize_value__str() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct S<'a> {
            x: &'a str,
        };

        let v = vec![InterfaceValue::String("foo".to_string())];
        let input: S = from_values(&v).unwrap();
        let output = S { x: "foo" };

        assert_eq!(input, output);
    }

    value!(test_deserialize_value__i32, I32, i32, 42);
    value!(test_deserialize_value__i64, I64, i64, 42);

    #[test]
    #[allow(non_snake_case)]
    fn test_deserialize_value__record() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct S {
            x: i32,
            y: i64,
        };

        #[derive(Deserialize, Debug, PartialEq)]
        struct T {
            s: S,
        };

        let v = vec![InterfaceValue::Record(vec![
            InterfaceValue::I32(1),
            InterfaceValue::I64(2),
        ])];
        let input: T = from_values(&v).unwrap();
        let output = T {
            s: S { x: 1, y: 2 },
        };

        assert_eq!(input, output);
    }
}
