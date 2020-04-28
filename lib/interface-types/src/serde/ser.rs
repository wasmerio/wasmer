//! Provides a serializer from Rust value to WIT values.

use crate::{values::InterfaceValue, vec1::Vec1};
use serde::{ser, Serialize};
use std::fmt::{self, Display};

/// Serialize a type `T` that implements the `Serialize` trait to an
/// `InterfaceValue`.
///
/// This is not a requirement to use WIT, but Serde provides an even
/// nicer API to the user to send its complex types to WIT.
///
/// # Example
///
/// ```rust
/// use wasmer_interface_types::{
///     values::{InterfaceValue, to_interface_value},
///     vec1::Vec1,
/// };
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// struct S(i32, i64);
///
/// #[derive(Serialize)]
/// struct T {
///     x: String,
///     s: S,
///     y: f32,
/// };
///
/// let input = T {
///     x: "abc".to_string(),
///     s: S(1, 2),
///     y: 3.,
/// };
///
/// assert_eq!(
///     to_interface_value(&input).unwrap(),
///     InterfaceValue::Record(Vec1::new(vec![
///         InterfaceValue::String("abc".to_string()),
///         InterfaceValue::Record(Vec1::new(vec![InterfaceValue::I32(1), InterfaceValue::I64(2)]).unwrap()),
///         InterfaceValue::F32(3.),
///     ]).unwrap()),
/// );
/// ```
pub fn to_interface_value<T>(value: &T) -> Result<InterfaceValue, SerializeError>
where
    T: Serialize,
{
    let mut serializer = Serializer::new();
    value.serialize(&mut serializer)?;

    if serializer.values.len() != 1 {
        Err(SerializeError::TransformationNotFinished)
    } else {
        let mut first_values = serializer.values.pop().unwrap(); // this `unwrap` is safe because we are sure the length is 1.

        if first_values.len() != 1 {
            Err(SerializeError::TransformationNotFinished)
        } else {
            let first_value = first_values.pop().unwrap(); // this `unwrap` is safe because we are sure the length is 1.

            Ok(first_value)
        }
    }
}

/// The serializer.
struct Serializer {
    values: Vec<Vec<InterfaceValue>>,
}

impl Serializer {
    fn new() -> Self {
        Self {
            values: vec![vec![]],
        }
    }

    fn last(&mut self) -> &mut Vec<InterfaceValue> {
        self.values.last_mut().unwrap()
    }

    fn push_with_capacity(&mut self, capacity: usize) {
        self.values.push(Vec::with_capacity(capacity));
    }

    fn pop(&mut self) -> Result<Vec<InterfaceValue>, SerializeError> {
        // The first `vec` contains the final result. It is forbidden
        // to `pop` it as is.
        if self.values.len() < 2 {
            Err(SerializeError::InternalValuesCorrupted)
        } else {
            Ok(self.values.pop().unwrap()) // this `unwrap` is safe before `self.values` contains at least 2 items
        }
    }
}

/// Represents an error while serializing.
#[derive(Clone, Debug, PartialEq)]
pub enum SerializeError {
    /// The serialization still has pending values internally.
    TransformationNotFinished,

    /// The internal values have been corrupted during the
    /// serialization.
    InternalValuesCorrupted,

    /// A record must contain at least one field.
    RecordNeedsAtLeastOneField,

    /// Arbitrary message.
    Message(String),
}

impl ser::Error for SerializeError {
    fn custom<T: Display>(msg: T) -> Self {
        Self::Message(msg.to_string())
    }
}

impl Display for SerializeError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::TransformationNotFinished => write!(
                formatter,
                "serialization still has pending values internally, something incorrect happened"
            ),
            Self::InternalValuesCorrupted => write!(
                formatter,
                "the internal values have been corrutped during the serialization"
            ),
            Self::RecordNeedsAtLeastOneField => write!(
                formatter,
                "a record must contain at least one field, zero given"
            ),
            Self::Message(ref msg) => write!(formatter, "{}", msg),
        }
    }
}

impl std::error::Error for SerializeError {}

impl<'a> ser::Serializer for &'a mut Serializer {
    type Ok = ();
    type Error = SerializeError;

    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    fn serialize_bool(self, _value: bool) -> Result<Self::Ok, Self::Error> {
        unimplemented!("`bool` is not supported by WIT for the moment.")
    }

    fn serialize_i8(self, value: i8) -> Result<Self::Ok, Self::Error> {
        self.last().push(value.into());

        Ok(())
    }

    fn serialize_i16(self, value: i16) -> Result<Self::Ok, Self::Error> {
        self.last().push(value.into());

        Ok(())
    }

    fn serialize_i32(self, value: i32) -> Result<Self::Ok, Self::Error> {
        self.last().push(value.into());

        Ok(())
    }

    fn serialize_i64(self, value: i64) -> Result<Self::Ok, Self::Error> {
        self.last().push(value.into());

        Ok(())
    }

    fn serialize_u8(self, value: u8) -> Result<Self::Ok, Self::Error> {
        self.last().push(value.into());

        Ok(())
    }

    fn serialize_u16(self, value: u16) -> Result<Self::Ok, Self::Error> {
        self.last().push(value.into());

        Ok(())
    }

    fn serialize_u32(self, value: u32) -> Result<Self::Ok, Self::Error> {
        self.last().push(value.into());

        Ok(())
    }

    fn serialize_u64(self, value: u64) -> Result<Self::Ok, Self::Error> {
        self.last().push(value.into());

        Ok(())
    }

    fn serialize_f32(self, value: f32) -> Result<Self::Ok, Self::Error> {
        self.last().push(value.into());

        Ok(())
    }

    fn serialize_f64(self, value: f64) -> Result<Self::Ok, Self::Error> {
        self.last().push(value.into());

        Ok(())
    }

    fn serialize_char(self, _value: char) -> Result<Self::Ok, Self::Error> {
        todo!("`char` is not supported by WIT for the moment.")
    }

    fn serialize_str(self, value: &str) -> Result<Self::Ok, Self::Error> {
        self.last().push(value.to_owned().into());

        Ok(())
    }

    fn serialize_bytes(self, _value: &[u8]) -> Result<Self::Ok, Self::Error> {
        todo!("`bytes` is not supported by WIT for the moment.")
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.serialize_unit()
    }

    fn serialize_some<T>(self, _value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        todo!("`some` is not supported by WIT for the moment.")
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        todo!("`unit` is not supported by WIT for the moment.")
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        todo!("`unit_struct` is not supported by WIT for the moment.")
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        todo!("`unit_variant` is not supported by WIT for the moment.")
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        todo!("`newtype_variant` is not supported by WIT for the moment.")
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        todo!("`seq` is not supported by WIT for the moment.")
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        todo!("`tuple` is not supported by WIT for the moment.")
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        self.push_with_capacity(len);

        Ok(self)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        todo!("`tuple_variant` is not supported by WIT for the moment.")
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        todo!("`map` is not supported by WIT for the moment.")
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        self.push_with_capacity(len);

        Ok(self)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        todo!("`struct_variant` is not supported by WIT for the moment.")
    }
}

impl<'a> ser::SerializeSeq for &'a mut Serializer {
    type Ok = ();
    type Error = SerializeError;

    fn serialize_element<T>(&mut self, _value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        unimplemented!()
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }
}

impl<'a> ser::SerializeTuple for &'a mut Serializer {
    type Ok = ();
    type Error = SerializeError;

    fn serialize_element<T>(&mut self, _value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        unimplemented!()
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }
}

impl<'a> ser::SerializeTupleStruct for &'a mut Serializer {
    type Ok = ();
    type Error = SerializeError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        let record = InterfaceValue::Record(
            Vec1::new(self.pop()?).map_err(|_| Self::Error::RecordNeedsAtLeastOneField)?,
        );
        self.last().push(record);

        Ok(())
    }
}

impl<'a> ser::SerializeTupleVariant for &'a mut Serializer {
    type Ok = ();
    type Error = SerializeError;

    fn serialize_field<T>(&mut self, _value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        unimplemented!()
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }
}

impl<'a> ser::SerializeMap for &'a mut Serializer {
    type Ok = ();
    type Error = SerializeError;

    fn serialize_key<T>(&mut self, _key: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        unimplemented!()
    }

    fn serialize_value<T>(&mut self, _value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        unimplemented!()
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }
}

impl<'a> ser::SerializeStruct for &'a mut Serializer {
    type Ok = ();
    type Error = SerializeError;

    fn serialize_field<T>(&mut self, _key: &'static str, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        let record = InterfaceValue::Record(
            Vec1::new(self.pop()?).map_err(|_| Self::Error::RecordNeedsAtLeastOneField)?,
        );
        self.last().push(record);

        Ok(())
    }
}

impl<'a> ser::SerializeStructVariant for &'a mut Serializer {
    type Ok = ();
    type Error = SerializeError;

    fn serialize_field<T>(
        &mut self,
        _key: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        unimplemented!()
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! serialize_value {
        ($test_name:ident, $ty:ident, $variant:ident, $value:expr) => {
            #[test]
            #[allow(non_snake_case)]
            fn $test_name() {
                let input: $ty = $value;
                let output = InterfaceValue::$variant($value);

                assert_eq!(to_interface_value(&input).unwrap(), output);
            }
        };
    }

    serialize_value!(test_serialize_value__s8, i8, S8, 42);
    serialize_value!(test_serialize_value__s16, i16, S16, 42);
    serialize_value!(test_serialize_value__i32, i32, I32, 42);
    serialize_value!(test_serialize_value__i64, i64, I64, 42);
    serialize_value!(test_serialize_value__u8, u8, U8, 42);
    serialize_value!(test_serialize_value__u16, u16, U16, 42);
    serialize_value!(test_serialize_value__u32, u32, U32, 42);
    serialize_value!(test_serialize_value__u64, u64, U64, 42);
    serialize_value!(test_serialize_value__f32, f32, F32, 42.);
    serialize_value!(test_serialize_value__f64, f32, F32, 42.);
    serialize_value!(
        test_serialize_value__string,
        String,
        String,
        "foo".to_string()
    );

    #[test]
    #[allow(non_snake_case)]
    fn test_serialize_value__newtype_struct() {
        #[derive(Serialize)]
        struct S(i8);

        let input = S(42);
        let output = InterfaceValue::S8(42);

        assert_eq!(to_interface_value(&input).unwrap(), output);
    }

    #[test]
    #[allow(non_snake_case)]
    fn test_serialize_value__tuple_struct() {
        #[derive(Serialize)]
        struct S(i8, f32);

        let input = S(7, 42.);
        let output = InterfaceValue::Record(vec1![InterfaceValue::S8(7), InterfaceValue::F32(42.)]);

        assert_eq!(to_interface_value(&input).unwrap(), output);
    }

    #[test]
    #[allow(non_snake_case)]
    fn test_serialize_value__struct() {
        #[derive(Serialize)]
        struct S {
            x: i8,
            y: f32,
        }

        let input = S { x: 7, y: 42. };
        let output = InterfaceValue::Record(vec1![InterfaceValue::S8(7), InterfaceValue::F32(42.)]);

        assert_eq!(to_interface_value(&input).unwrap(), output);
    }

    #[test]
    #[allow(non_snake_case)]
    fn test_serialize_value__struct_nested() {
        #[derive(Serialize)]
        struct Point {
            x: i32,
            y: i32,
            z: i32,
        }

        #[derive(Serialize)]
        struct Line {
            p1: Point,
            p2: Point,
        }

        let input = Line {
            p1: Point { x: 1, y: 2, z: 3 },
            p2: Point { x: 4, y: 5, z: 6 },
        };
        let output = InterfaceValue::Record(vec1![
            InterfaceValue::Record(vec1![
                InterfaceValue::I32(1),
                InterfaceValue::I32(2),
                InterfaceValue::I32(3),
            ]),
            InterfaceValue::Record(vec1![
                InterfaceValue::I32(4),
                InterfaceValue::I32(5),
                InterfaceValue::I32(6),
            ]),
        ]);

        assert_eq!(to_interface_value(&input).unwrap(), output);
    }
}
