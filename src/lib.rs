mod parsed_document;

use std::{borrow::Cow, sync::Arc};

use bson::{
    Binary, Bson, DateTime, Decimal128, Document, JavaScriptCodeWithScope, RawArray, RawBinaryRef,
    RawBsonRef, RawDbPointerRef, RawDocument, RawJavaScriptCodeWithScopeRef, RawRegexRef, Regex,
    Timestamp, oid::ObjectId, spec::ElementType,
};

use bytes::BufMut;
pub use parsed_document::ParsedDocument;

fn raw_cstr_len(s: &str) -> usize {
    s.len() + 1
}

fn put_raw_cstr(s: &str, buf: &mut impl BufMut) -> Result<(), bson::ser::Error> {
    if s.as_bytes().contains(&0) {
        Err(bson::ser::Error::InvalidCString(s.to_owned()))
    } else {
        buf.put_slice(s.as_bytes());
        buf.put_u8(0);
        Ok(())
    }
}

fn raw_str_len(s: &str) -> usize {
    4 + s.len() + 1
}

fn put_raw_str(s: &str, buf: &mut impl BufMut) {
    buf.put_i32_le(
        (s.len() + 1)
            .try_into()
            .expect("len validated before serialization"),
    );
    buf.put_slice(s.as_bytes());
    buf.put_u8(0);
}

/// A BSON Value that is mutable.
///
/// `Document` and `Array` types may refer to owned/unowned raw BSON types or to a parsed
/// representation that allows mutation of individual elements. For other variable size types we
/// either use a [`Cow`] or wrap owned + reference types. Fixed size types use the same inline
/// representation as they do in [`bson::Bson`] and [`bson::RawBson`].
#[derive(Clone, Debug)]
pub enum MutableValue<'a> {
    Double(f64),
    String(Cow<'a, str>),
    Document(MutableDocument<'a>),
    Array(MutableArray<'a>),
    Binary(MutableBinary<'a>),
    Undefined,
    ObjectId(ObjectId),
    Boolean(bool),
    DateTime(DateTime),
    Null,
    RegularExpression(MutableRegex<'a>),
    /// DbPointers cannot be mutated using the `bson` crate.
    DbPointer(RawDbPointerRef<'a>),
    JavaScriptCode(Cow<'a, str>),
    Symbol(Cow<'a, str>),
    JavaScriptCodeWithScope(MutableJavaScriptCodeWithScope<'a>),
    Int32(i32),
    Timestamp(Timestamp),
    Int64(i64),
    Decimal128(Decimal128),
    MinKey,
    MaxKey,
}

impl<'a> MutableValue<'a> {
    /// Get the [`bson::spec::ElementType`] used on the wire for a value type.
    pub fn element_type(&self) -> ElementType {
        match self {
            Self::Double(_) => ElementType::Double,
            Self::String(_) => ElementType::String,
            Self::Document(_) => ElementType::EmbeddedDocument,
            Self::Array(_) => ElementType::Array,
            Self::Binary(_) => ElementType::Binary,
            Self::Undefined => ElementType::Undefined,
            Self::ObjectId(_) => ElementType::ObjectId,
            Self::Boolean(_) => ElementType::Boolean,
            Self::DateTime(_) => ElementType::DateTime,
            Self::Null => ElementType::Null,
            Self::RegularExpression(_) => ElementType::RegularExpression,
            Self::DbPointer(_) => ElementType::DbPointer,
            Self::JavaScriptCode(_) => ElementType::JavaScriptCode,
            Self::Symbol(_) => ElementType::Symbol,
            Self::JavaScriptCodeWithScope(_) => ElementType::JavaScriptCodeWithScope,
            Self::Int32(_) => ElementType::Int32,
            Self::Timestamp(_) => ElementType::Timestamp,
            Self::Int64(_) => ElementType::Int64,
            Self::Decimal128(_) => ElementType::Decimal128,
            Self::MinKey => ElementType::MinKey,
            Self::MaxKey => ElementType::MaxKey,
        }
    }

    /// Returns the raw binary coded length of this value.
    fn raw_len(&self) -> usize {
        match self {
            Self::Double(_) => 8,
            Self::String(v) => raw_str_len(v),
            Self::Document(v) => v.raw_len(),
            Self::Array(v) => v.raw_len(),
            Self::Binary(v) => v.raw_len(),
            Self::Undefined => 0,
            Self::ObjectId(_) => 12,
            Self::Boolean(_) => 1,
            Self::DateTime(_) => 8,
            Self::Null => 0,
            Self::RegularExpression(v) => v.raw_len(),
            // No visibility into DbPointer components. Could still be fixed, it's just annoying.
            Self::DbPointer(_) => unimplemented!(),
            Self::JavaScriptCode(v) => raw_str_len(v),
            Self::Symbol(v) => raw_str_len(v),
            Self::JavaScriptCodeWithScope(v) => v.raw_len(),
            Self::Int32(_) => 4,
            Self::Timestamp(_) => 8,
            Self::Int64(_) => 8,
            Self::Decimal128(_) => 16,
            Self::MinKey => 0,
            Self::MaxKey => 0,
        }
    }

    fn put(&self, buf: &mut impl BufMut) -> Result<(), bson::ser::Error> {
        match self {
            Self::Double(v) => buf.put_f64_le(*v),
            Self::String(v) => put_raw_str(v, buf),
            Self::Document(v) => v.put(buf)?,
            Self::Array(v) => v.put(buf)?,
            Self::Binary(v) => v.put(buf),
            Self::Undefined => (),
            Self::ObjectId(v) => buf.put_slice(&v.bytes()),
            Self::Boolean(v) => buf.put_u8((*v).into()),
            Self::DateTime(v) => buf.put_i64_le(v.timestamp_millis()),
            Self::Null => (),
            Self::RegularExpression(v) => v.put(buf)?,
            // No visibility into DbPointer components. Could still be fixed, it's just annoying.
            Self::DbPointer(_) => unimplemented!(),
            Self::JavaScriptCode(v) => put_raw_str(v, buf),
            Self::Symbol(v) => put_raw_str(v, buf),
            Self::JavaScriptCodeWithScope(v) => v.put(buf)?,
            Self::Int32(v) => buf.put_i32_le(*v),
            Self::Timestamp(v) => {
                buf.put_u32_le(v.increment);
                buf.put_u32_le(v.time);
            }
            Self::Int64(v) => buf.put_i64_le(*v),
            Self::Decimal128(v) => buf.put_slice(&v.bytes()),
            Self::MinKey => (),
            Self::MaxKey => (),
        };
        Ok(())
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Double(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(v) => Some(v.as_ref()),
            _ => None,
        }
    }

    pub fn as_doc(&self) -> Option<&MutableDocument<'a>> {
        match self {
            Self::Document(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_doc_mut(&mut self) -> Option<&mut MutableDocument<'a>> {
        match self {
            Self::Document(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&MutableArray<'a>> {
        match self {
            Self::Array(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut MutableArray<'a>> {
        match self {
            Self::Array(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_binary(&self) -> Option<&MutableBinary<'a>> {
        match self {
            Self::Binary(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_binary_mut(&mut self) -> Option<&mut MutableBinary<'a>> {
        match self {
            Self::Binary(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_object_id(&self) -> Option<ObjectId> {
        match self {
            Self::ObjectId(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_date_time(&self) -> Option<DateTime> {
        match self {
            Self::DateTime(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_null(&self) -> Option<()> {
        match self {
            Self::Null => Some(()),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::Int32(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_timestamp(&self) -> Option<Timestamp> {
        match self {
            Self::Timestamp(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int64(v) => Some(*v),
            _ => None,
        }
    }
}

impl<'a> From<RawBsonRef<'a>> for MutableValue<'a> {
    fn from(value: RawBsonRef<'a>) -> Self {
        match value {
            RawBsonRef::Double(v) => Self::Double(v),
            RawBsonRef::String(v) => Self::String(v.into()),
            RawBsonRef::Document(v) => Self::Document(v.into()),
            RawBsonRef::Array(v) => Self::Array(v.into()),
            RawBsonRef::Binary(v) => Self::Binary(v.into()),
            RawBsonRef::Undefined => Self::Undefined,
            RawBsonRef::ObjectId(v) => Self::ObjectId(v),
            RawBsonRef::Boolean(v) => Self::Boolean(v),
            RawBsonRef::DateTime(v) => Self::DateTime(v),
            RawBsonRef::Null => Self::Null,
            RawBsonRef::RegularExpression(v) => Self::RegularExpression(v.into()),
            RawBsonRef::DbPointer(v) => Self::DbPointer(v),
            RawBsonRef::JavaScriptCode(v) => Self::JavaScriptCode(v.into()),
            RawBsonRef::Symbol(v) => Self::Symbol(v.into()),
            RawBsonRef::JavaScriptCodeWithScope(v) => Self::JavaScriptCodeWithScope(v.into()),
            RawBsonRef::Int32(v) => Self::Int32(v),
            RawBsonRef::Timestamp(v) => Self::Timestamp(v),
            RawBsonRef::Int64(v) => Self::Int64(v),
            RawBsonRef::Decimal128(v) => Self::Decimal128(v),
            RawBsonRef::MinKey => Self::MinKey,
            RawBsonRef::MaxKey => Self::MaxKey,
        }
    }
}

impl<'a, T: Clone + Into<MutableValue<'a>>> From<&T> for MutableValue<'a> {
    fn from(value: &T) -> Self {
        value.clone().into()
    }
}

impl From<Bson> for MutableValue<'_> {
    fn from(value: Bson) -> Self {
        match value {
            Bson::Double(v) => Self::Double(v),
            Bson::String(v) => Self::String(v.into()),
            Bson::Document(v) => Self::Document(v.into()),
            Bson::Array(v) => Self::Array(v.into()),
            Bson::Binary(v) => Self::Binary(v.into()),
            Bson::Undefined => Self::Undefined,
            Bson::ObjectId(v) => Self::ObjectId(v),
            Bson::Boolean(v) => Self::Boolean(v),
            Bson::DateTime(v) => Self::DateTime(v),
            Bson::Null => Self::Null,
            Bson::RegularExpression(v) => Self::RegularExpression(v.into()),
            Bson::DbPointer(_) => unimplemented!(),
            Bson::JavaScriptCode(v) => Self::JavaScriptCode(v.into()),
            Bson::Symbol(v) => Self::Symbol(v.into()),
            Bson::JavaScriptCodeWithScope(v) => Self::JavaScriptCodeWithScope(v.into()),
            Bson::Int32(v) => Self::Int32(v),
            Bson::Timestamp(v) => Self::Timestamp(v),
            Bson::Int64(v) => Self::Int64(v),
            Bson::Decimal128(v) => Self::Decimal128(v),
            Bson::MinKey => Self::MinKey,
            Bson::MaxKey => Self::MaxKey,
        }
    }
}

impl From<f64> for MutableValue<'_> {
    fn from(value: f64) -> Self {
        Self::Double(value)
    }
}

impl From<String> for MutableValue<'_> {
    fn from(value: String) -> Self {
        Self::String(value.into())
    }
}

impl From<&str> for MutableValue<'_> {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}

impl<'a> From<ParsedDocument<'a>> for MutableValue<'a> {
    fn from(value: ParsedDocument<'a>) -> Self {
        Self::Document(value.into())
    }
}

impl<'a> From<Vec<MutableValue<'a>>> for MutableValue<'a> {
    fn from(value: Vec<MutableValue<'a>>) -> Self {
        Self::Array(value.into())
    }
}

impl<'a> From<MutableBinary<'a>> for MutableValue<'a> {
    fn from(value: MutableBinary<'a>) -> Self {
        Self::Binary(value)
    }
}

impl From<Binary> for MutableValue<'_> {
    fn from(value: Binary) -> Self {
        Self::Binary(value.into())
    }
}

impl From<ObjectId> for MutableValue<'_> {
    fn from(value: ObjectId) -> Self {
        Self::ObjectId(value)
    }
}

impl From<bool> for MutableValue<'_> {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<DateTime> for MutableValue<'_> {
    fn from(value: DateTime) -> Self {
        Self::DateTime(value)
    }
}

impl<'a> From<MutableRegex<'a>> for MutableValue<'a> {
    fn from(value: MutableRegex<'a>) -> Self {
        Self::RegularExpression(value)
    }
}

impl From<Regex> for MutableValue<'_> {
    fn from(value: Regex) -> Self {
        Self::RegularExpression(value.into())
    }
}

impl<'a> From<MutableJavaScriptCodeWithScope<'a>> for MutableValue<'a> {
    fn from(value: MutableJavaScriptCodeWithScope<'a>) -> Self {
        Self::JavaScriptCodeWithScope(value)
    }
}

impl From<i32> for MutableValue<'_> {
    fn from(value: i32) -> Self {
        Self::Int32(value)
    }
}

impl From<Timestamp> for MutableValue<'_> {
    fn from(value: Timestamp) -> Self {
        Self::Timestamp(value)
    }
}

impl From<i64> for MutableValue<'_> {
    fn from(value: i64) -> Self {
        Self::Int64(value)
    }
}

impl From<Decimal128> for MutableValue<'_> {
    fn from(value: Decimal128) -> Self {
        Self::Decimal128(value)
    }
}

/// Contains either an encoded BSON document or a [`ParsedDocument`] that has decoded all of the
/// key and [`MutableValue`] pairs for fast access and to allow mutation.
///
/// NB: while [bson::RawDocument] provides keyed access, it does so by decoding from the beginning
/// of the document so it is often unwise to use [bson::RawDocument::get] and friends.
#[derive(Clone, Debug)]
pub enum MutableDocument<'a> {
    Borrowed(&'a RawDocument),
    Owned(ParsedDocument<'a>),
}

impl<'a> MutableDocument<'a> {
    /// Try to convert the representation to a [`ParsedDocument`] from a [`RawDocument`](bson::RawDocument)
    /// if necessary.
    ///
    /// May fail with a raw BSON parsing error.
    // TODO: this should be a impl TryFrom<MutableDocument> for ParsedDocument
    pub fn try_into_parsed(self) -> Result<Self, bson::raw::Error> {
        match self {
            Self::Owned(p) => Ok(p.into()),
            Self::Borrowed(e) => ParsedDocument::try_from(e).map(Self::from),
        }
    }

    /// Try to convert the representation to a [`ParsedArray`] from a [`RawArray`](bson::RawArray)
    /// if necessary and return a mutable reference to the `ParsedArray`.
    ///
    /// May fail with a raw BSON parsing error.
    pub fn to_parsed(&mut self) -> Result<&mut ParsedDocument<'a>, bson::raw::Error> {
        if let Self::Borrowed(e) = self {
            *self = Self::Owned(ParsedDocument::try_from(e as &RawDocument)?);
        }
        match self {
            Self::Borrowed(_) => unreachable!(),
            Self::Owned(p) => Ok(p),
        }
    }

    fn raw_len(&self) -> usize {
        match self {
            Self::Borrowed(e) => e.as_bytes().len(),
            Self::Owned(p) => p.raw_len(),
        }
    }

    fn put(&self, buf: &mut impl BufMut) -> Result<(), bson::ser::Error> {
        match self {
            Self::Borrowed(e) => {
                buf.put_slice(e.as_bytes());
                Ok(())
            }
            Self::Owned(p) => p.put(buf),
        }
    }

    /// Produce an encoded raw document.
    pub fn to_vec(&self) -> Result<Vec<u8>, bson::ser::Error> {
        // TODO: cache the raw length in all MutableValues where length computation is non-trivial.
        // Leaving this uncache is a problem because we will call raw_len() twice on the root
        // document: once to size the output buffer and once to emit the buffer, and this will be
        // done for all mutable objects down the tree. The borrow checker should help us here as
        // all mutable methods can ensure that the cached value is invalidated.
        // This is easy for ParsedDocument, MutableArray::Owned would need a lot of work.
        let len = self.raw_len();
        if len >= 32 << 20 {
            return Err(bson::ser::Error::Io(Arc::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Exceeded max document length",
            ))));
        }
        let mut buf = Vec::with_capacity(len);
        self.put(&mut buf).map(|_| buf)
    }
}

impl<'a> From<&'a RawDocument> for MutableDocument<'a> {
    fn from(value: &'a RawDocument) -> Self {
        Self::Borrowed(value)
    }
}

impl<'a> From<ParsedDocument<'a>> for MutableDocument<'a> {
    fn from(value: ParsedDocument<'a>) -> Self {
        Self::Owned(value)
    }
}

impl From<Document> for MutableDocument<'_> {
    fn from(value: Document) -> Self {
        Self::Owned(value.into())
    }
}

/// Contains either an encoded BSON array or a [`Vec`] of [`MutableValue`]s decoded from a raw array
/// to allow fast access and mutation.
///
/// NB: while [bson::RawArray] provides indexed access, it does so by decoding from the beginning
/// of the array so it is often unwise to use [bson::RawArray::get] and friends.
#[derive(Clone, Debug)]
pub enum MutableArray<'a> {
    Borrowed(&'a RawArray),
    Owned(Vec<MutableValue<'a>>),
}

impl<'a> MutableArray<'a> {
    /// Try to convert the representation to a [`ParsedArray`] from a [`RawArray`](bson::RawArray)
    /// if necessary.
    ///
    /// May fail with a raw BSON parsing error.
    pub fn try_into_parsed(self) -> Result<Self, bson::raw::Error> {
        match self {
            Self::Owned(p) => Ok(p.into()),
            Self::Borrowed(e) => Self::encoded_to_parsed(e).map(Self::from),
        }
    }

    /// Try to convert the representation to a [`ParsedArray`] from a [`RawArray`](bson::RawArray)
    /// if necessary and return a mutable reference to the `ParsedArray`.
    ///
    /// May fail with a raw BSON parsing error.
    pub fn to_parsed(&mut self) -> Result<&mut Vec<MutableValue<'a>>, bson::raw::Error> {
        if let Self::Borrowed(e) = self {
            *self = Self::Owned(Self::encoded_to_parsed(e as &RawArray)?);
        }
        match self {
            Self::Borrowed(_) => unreachable!(),
            Self::Owned(p) => Ok(p),
        }
    }

    fn encoded_to_parsed(raw: &RawArray) -> Result<Vec<MutableValue<'_>>, bson::raw::Error> {
        let mut values = vec![];
        for e in raw.into_iter() {
            values.push((e?).into());
        }
        Ok(values)
    }

    fn raw_len(&self) -> usize {
        match self {
            Self::Borrowed(e) => e.as_bytes().len(),
            Self::Owned(p) => {
                // TODO: more efficient way of counting key bytes.
                p.iter()
                    .enumerate()
                    .map(|(i, v)| 1 + raw_cstr_len(itoa::Buffer::new().format(i)) + v.raw_len())
                    .sum::<usize>()
                    // doc length
                    + 4usize
                    // doc null terminator
                    + 1usize
            }
        }
    }

    fn put(&self, buf: &mut impl BufMut) -> Result<(), bson::ser::Error> {
        match self {
            Self::Borrowed(e) => {
                buf.put_slice(e.as_bytes());
                Ok(())
            }
            Self::Owned(p) => {
                buf.put_i32_le(
                    self.raw_len()
                        .try_into()
                        .expect("message len validated before put"),
                );
                for (i, v) in p.iter().enumerate() {
                    buf.put_u8(v.element_type() as u8);
                    put_raw_cstr(itoa::Buffer::new().format(i), buf)
                        .expect("itoa does not contain nulls");
                    v.put(buf)?;
                }
                buf.put_u8(0);
                Ok(())
            }
        }
    }
}

impl<'a> From<&'a RawArray> for MutableArray<'a> {
    fn from(value: &'a RawArray) -> Self {
        Self::Borrowed(value)
    }
}

impl<'a> From<Vec<MutableValue<'a>>> for MutableArray<'a> {
    fn from(value: Vec<MutableValue<'a>>) -> Self {
        Self::Owned(value)
    }
}

impl From<Vec<Bson>> for MutableArray<'_> {
    fn from(value: Vec<Bson>) -> Self {
        Self::Owned(value.into_iter().map(MutableValue::from).collect())
    }
}

#[derive(Clone, Debug)]
pub enum MutableBinary<'a> {
    Borrowed(RawBinaryRef<'a>),
    Owned(Binary),
}

impl MutableBinary<'_> {
    fn raw_len(&self) -> usize {
        let bytes = match self {
            Self::Borrowed(v) => v.bytes,
            Self::Owned(v) => v.bytes.as_ref(),
        };
        // length of the byte string + 4 bytes for length + 1 byte for subtype.
        4 + bytes.len() + 1
    }

    fn put(&self, buf: &mut impl BufMut) {
        let (bytes, subtype) = match self {
            Self::Borrowed(v) => (v.bytes, v.subtype),
            Self::Owned(v) => (v.bytes.as_ref(), v.subtype),
        };
        buf.put_i32_le(
            bytes
                .len()
                .try_into()
                .expect("message len verified before put"),
        );
        buf.put_u8(subtype.into());
        buf.put_slice(bytes);
    }
}

impl<'a> From<RawBinaryRef<'a>> for MutableBinary<'a> {
    fn from(value: RawBinaryRef<'a>) -> Self {
        Self::Borrowed(value)
    }
}

impl From<Binary> for MutableBinary<'_> {
    fn from(value: Binary) -> Self {
        Self::Owned(value)
    }
}

#[derive(Clone, Debug)]
pub enum MutableRegex<'a> {
    Borrowed(RawRegexRef<'a>),
    Owned(Regex),
}

impl MutableRegex<'_> {
    fn raw_len(&self) -> usize {
        let (pattern, options) = self.parts();
        raw_cstr_len(pattern) + raw_cstr_len(options)
    }

    fn put(&self, buf: &mut impl BufMut) -> Result<(), bson::ser::Error> {
        let (pattern, options) = self.parts();
        put_raw_cstr(pattern, buf)?;
        put_raw_cstr(options, buf)
    }

    fn parts(&self) -> (&str, &str) {
        match self {
            Self::Borrowed(v) => (v.pattern, v.options),
            Self::Owned(v) => (v.pattern.as_ref(), v.options.as_ref()),
        }
    }
}

impl<'a> From<RawRegexRef<'a>> for MutableRegex<'a> {
    fn from(value: RawRegexRef<'a>) -> Self {
        Self::Borrowed(value)
    }
}

impl From<Regex> for MutableRegex<'_> {
    fn from(value: Regex) -> Self {
        Self::Owned(value)
    }
}

#[derive(Clone, Debug)]
pub enum MutableJavaScriptCodeWithScope<'a> {
    Borrowed(RawJavaScriptCodeWithScopeRef<'a>),
    Owned(JavaScriptCodeWithScope),
}

impl MutableJavaScriptCodeWithScope<'_> {
    fn raw_len(&self) -> usize {
        match self {
            Self::Borrowed(v) => 4 + raw_str_len(v.code) + v.scope.as_bytes().len(),
            Self::Owned(v) => {
                // TODO: the owned object should be code + ParsedDocument scoped.
                4 + raw_str_len(&v.code) + bson::to_vec(&v.scope).unwrap().len()
            }
        }
    }

    fn put(&self, buf: &mut impl BufMut) -> Result<(), bson::ser::Error> {
        match self {
            Self::Borrowed(v) => {
                buf.put_i32_le(
                    (raw_str_len(v.code) + v.scope.as_bytes().len() + 4)
                        .try_into()
                        .expect("document length verified before put()"),
                );
                put_raw_str(v.code, buf);
                buf.put_slice(v.scope.as_bytes());
            }
            Self::Owned(v) => {
                let encoded_scope = bson::to_vec(&v.scope)?;
                buf.put_i32_le(
                    (raw_str_len(&v.code) + encoded_scope.len() + 4)
                        .try_into()
                        .expect("document length verified before put()"),
                );
                put_raw_str(&v.code, buf);
                buf.put_slice(&encoded_scope);
            }
        };
        Ok(())
    }
}

impl<'a> From<RawJavaScriptCodeWithScopeRef<'a>> for MutableJavaScriptCodeWithScope<'a> {
    fn from(value: RawJavaScriptCodeWithScopeRef<'a>) -> Self {
        Self::Borrowed(value)
    }
}

impl From<JavaScriptCodeWithScope> for MutableJavaScriptCodeWithScope<'_> {
    fn from(value: JavaScriptCodeWithScope) -> Self {
        Self::Owned(value)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {}
}
