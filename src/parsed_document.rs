use std::{borrow::Cow, ops::Index};

use bson::{Document, RawDocument};
use bytes::BufMut;
use indexmap::IndexMap;

use crate::{MutableValue, put_raw_cstr, raw_cstr_len};

#[derive(Default, Clone, Debug)]
pub struct ParsedDocument<'a>(IndexMap<Cow<'a, str>, MutableValue<'a>>);

impl<'a> ParsedDocument<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert<V: Into<MutableValue<'static>>>(
        &mut self,
        key: impl Into<String>,
        value: V,
    ) -> Option<MutableValue<'_>> {
        self.0.insert(Cow::from(key.into()), value.into())
    }

    /// Remove key and return the value for that key if present.
    ///
    /// Runs in _O(n)_ time.
    pub fn remove(&mut self, key: impl AsRef<str>) -> Option<MutableValue<'a>> {
        self.0.shift_remove(key.as_ref())
    }

    pub fn clear(&mut self) {
        self.0.clear()
    }

    pub fn contains_key(&self, key: impl AsRef<str>) -> bool {
        self.0.contains_key(key.as_ref())
    }

    pub fn get(&self, key: impl AsRef<str>) -> Option<&MutableValue<'a>> {
        self.0.get(key.as_ref())
    }

    pub fn get_mut(&mut self, key: impl AsRef<str>) -> Option<&mut MutableValue<'a>> {
        self.0.get_mut(key.as_ref())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &MutableValue<'a>)> {
        self.0.iter().map(|(k, v)| (k.as_ref(), v))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&str, &mut MutableValue<'a>)> {
        self.0.iter_mut().map(|(k, v)| (k.as_ref(), v))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) fn raw_len(&self) -> usize {
        self.0
            .iter()
            // 1 byte for type, key, 1 byte null terminator, value.
            .map(|(k, v)| 1 + raw_cstr_len(k.as_ref()) + v.raw_len())
            .sum::<usize>()
            // 4 bytes for doc length, 1 byte for null terminator.
            + 4usize + 1usize
    }

    pub(super) fn put(&self, buf: &mut impl BufMut) -> Result<(), bson::ser::Error> {
        buf.put_i32_le(
            self.raw_len()
                .try_into()
                .expect("message len checked before put"),
        );
        for (k, v) in self.0.iter() {
            buf.put_u8(v.element_type() as u8);
            put_raw_cstr(k.as_ref(), buf)?;
            v.put(buf)?;
        }
        buf.put_u8(0);
        Ok(())
    }

    // TODO: keys()
    // TODO: values()
    // TODO: values_mut()
    // TODO: hash/btree map style entry()
}

impl<'a> TryFrom<&'a RawDocument> for ParsedDocument<'a> {
    type Error = bson::raw::Error;

    fn try_from(value: &'a RawDocument) -> Result<Self, Self::Error> {
        let mut fields = IndexMap::new();
        for e in value.iter() {
            let (k, v) = e?;
            fields.insert(k.into(), v.into());
        }
        Ok(Self(fields))
    }
}

impl From<Document> for ParsedDocument<'_> {
    fn from(value: Document) -> Self {
        Self(
            value
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        )
    }
}

impl<'a, S: AsRef<str>> Index<S> for ParsedDocument<'a> {
    type Output = MutableValue<'a>;

    fn index(&self, index: S) -> &Self::Output {
        &self.0[index.as_ref()]
    }
}

#[cfg(test)]
mod test {
    use bson::{
        Binary, Bson, DateTime, Decimal128, Document, JavaScriptCodeWithScope, RawDocumentBuf,
        Regex, Timestamp, bson, doc, oid::ObjectId, rawdoc, to_raw_document_buf,
    };

    use crate::MutableValue;

    use super::ParsedDocument;

    fn doc_to_vec(doc: &ParsedDocument<'_>) -> Vec<u8> {
        let mut out = vec![];
        doc.put(&mut out).unwrap();
        out
    }

    fn doc_all_types_owned() -> Document {
        let mut doc = Document::new();
        doc.insert("a", 1.0f64);
        doc.insert("b", "str");
        doc.insert("c", doc! { "text": "the quick brown fox" });
        doc.insert("d", bson!([1, "value"]));
        doc.insert(
            "e",
            Binary {
                subtype: bson::spec::BinarySubtype::Generic,
                bytes: vec![1u8, 2, 3],
            },
        );
        doc.insert("f", Bson::Undefined);
        doc.insert("g", ObjectId::from_bytes([0xae; 12]));
        doc.insert("h", true);
        doc.insert("i", DateTime::from_millis(1234567890));
        doc.insert("j", Bson::Null);
        doc.insert(
            "k",
            Regex {
                pattern: "foo.*".into(),
                options: "i".into(),
            },
        );
        doc.insert("m", Bson::JavaScriptCode("some code".into()));
        doc.insert("n", Bson::Symbol("symbol".into()));
        doc.insert(
            "o",
            JavaScriptCodeWithScope {
                code: "more code".into(),
                scope: doc! {"text": "jumped over the lazy dog"},
            },
        );
        doc.insert("p", 7i32);
        doc.insert(
            "q",
            Timestamp {
                time: 1234567890,
                increment: 2,
            },
        );
        doc.insert("r", 11i64);
        doc.insert("s", Decimal128::from_bytes([0xaf; 16]));
        doc.insert("t", Bson::MinKey);
        doc.insert("u", Bson::MaxKey);
        doc
    }

    fn doc_all_types_unowned() -> RawDocumentBuf {
        to_raw_document_buf(&doc_all_types_owned()).unwrap()
    }

    #[test]
    fn empty() {
        let doc = ParsedDocument::new();
        assert_eq!(doc.len(), 0);
        assert!(doc.is_empty());
        assert!(doc.iter().next().is_none());
        assert_eq!(doc.raw_len(), 5);
        assert_eq!(doc_to_vec(&doc), vec![5, 0, 0, 0, 0]);
    }

    #[test]
    fn all_types_owned() {
        let doc = ParsedDocument::from(doc_all_types_owned());
        assert_eq!(doc_to_vec(&doc), doc_all_types_unowned().as_bytes());
    }

    #[test]
    fn all_types_unowned() {
        let raw_doc = doc_all_types_unowned();
        let doc = ParsedDocument::try_from(raw_doc.as_ref()).unwrap();
        assert_eq!(doc_to_vec(&doc), doc_all_types_unowned().as_bytes());
    }

    #[test]
    fn contains_key() {
        let doc = ParsedDocument::from(doc_all_types_owned());
        assert!(doc.contains_key("e"));
        assert!(!doc.contains_key("z"));
    }

    #[test]
    fn get() {
        let doc = ParsedDocument::from(doc_all_types_owned());
        assert_eq!(doc.get("p").and_then(MutableValue::as_i32).unwrap(), 7);
        assert!(doc.get("z").is_none());
    }

    #[test]
    fn index() {
        let doc = ParsedDocument::from(doc_all_types_owned());
        assert_eq!(doc["p"].as_i32().unwrap(), 7);
        assert!(doc.get("z").is_none());
    }

    #[test]
    #[should_panic]
    fn index_invalid() {
        let doc = ParsedDocument::from(doc_all_types_owned());
        let _ = doc["z"];
    }

    #[test]
    fn insert_and_replace() {
        let mut doc = ParsedDocument::new();
        assert!(doc.insert("foo", 5).is_none());
        assert!(doc.insert("bar", "bat").is_none());
        assert_eq!(
            doc_to_vec(&doc),
            rawdoc! { "foo": 5, "bar": "bat" }.as_bytes()
        );

        assert!(doc.insert("foo", 16384).is_some());
        assert_eq!(
            doc_to_vec(&doc),
            rawdoc! { "foo": 16384, "bar": "bat" }.as_bytes()
        );
    }

    #[test]
    fn remove() {
        let mut doc = ParsedDocument::new();
        assert!(doc.insert("foo", 5).is_none());
        assert!(doc.insert("bar", "bat").is_none());
        assert_eq!(
            doc_to_vec(&doc),
            rawdoc! { "foo": 5, "bar": "bat" }.as_bytes()
        );

        assert!(doc.remove("foo").is_some());
        assert_eq!(doc_to_vec(&doc), rawdoc! { "bar": "bat" }.as_bytes());

        assert!(doc.remove("foo").is_none());
    }

    #[test]
    fn get_mut() {
        let mut doc = ParsedDocument::new();
        assert!(doc.insert("foo", 5).is_none());
        assert!(doc.get_mut("bar").is_none());

        let v = doc.get_mut("foo").expect("foo is present");
        if let MutableValue::Int32(i) = v {
            assert_eq!(*i, 5);
        } else {
            panic!("not an int32");
        }

        *v = "bar".into();
        assert_eq!(doc_to_vec(&doc), rawdoc! { "foo": "bar" }.as_bytes());
    }

    #[test]
    fn clear() {
        let mut doc = ParsedDocument::from(doc_all_types_owned());
        assert_eq!(doc_to_vec(&doc), doc_all_types_unowned().as_bytes());

        doc.clear();
        assert_eq!(doc_to_vec(&doc), vec![5, 0, 0, 0, 0]);
    }

    #[test]
    fn mutate_unowned() {
        let raw_doc = rawdoc! { "foo": "bar", "bat": 5 };
        let mut doc = ParsedDocument::try_from(raw_doc.as_ref()).unwrap();
        assert!(doc.insert("bat", true).is_some());
        assert!(doc.insert("quux", 7).is_none());
        assert_eq!(
            doc_to_vec(&doc),
            rawdoc! { "foo": "bar", "bat": true, "quux": 7 }.as_bytes()
        );
    }

    #[test]
    fn mutate_unowned_embedded_doc() {
        let raw_doc = rawdoc! { "foo": 5, "bar": { "id": 0, "score": 1.1 }};
        let mut doc = ParsedDocument::try_from(raw_doc.as_ref()).unwrap();
        let emb_doc = doc
            .get_mut("bar")
            .and_then(MutableValue::as_doc_mut)
            .unwrap()
            .to_parsed()
            .unwrap();
        assert!(emb_doc.insert("score", 2.1).is_some());
        assert!(emb_doc.insert("price", 7.17).is_none());
        assert_eq!(
            doc_to_vec(&doc),
            rawdoc! { "foo": 5, "bar": { "id": 0, "score": 2.1, "price": 7.17 }}.as_bytes()
        );
    }

    #[test]
    fn mutate_embedded_array() {
        let raw_doc = rawdoc! { "foo": 5, "vec": [0, "foo", 2, 3]};
        let mut doc = ParsedDocument::try_from(raw_doc.as_ref()).unwrap();
        let emb_array = doc
            .get_mut("vec")
            .and_then(MutableValue::as_array_mut)
            .unwrap()
            .to_parsed()
            .unwrap();
        emb_array[2] = "bar".into();
        emb_array.push(4.into());
        assert_eq!(
            doc_to_vec(&doc),
            rawdoc! { "foo": 5, "vec": [0, "foo", "bar", 3, 4]}.as_bytes()
        );
    }
}
