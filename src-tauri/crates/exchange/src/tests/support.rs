use std::{collections::VecDeque, marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use parking_lot::Mutex;

use super::super::*;

pub(super) struct QueueReader<P: Protocol, D: Direction> {
    values: VecDeque<Result<Option<P::Context>, Error>>,
    marker: PhantomData<D>,
}

impl<P: Protocol, D: Direction> QueueReader<P, D> {
    pub(super) fn contexts(values: impl IntoIterator<Item = P::Context>) -> Self {
        let mut values = values
            .into_iter()
            .map(|value| Ok(Some(value)))
            .collect::<VecDeque<_>>();
        values.push_back(Ok(None));
        Self {
            values,
            marker: PhantomData,
        }
    }
}

#[async_trait]
impl<P: Protocol, D: Direction> Reader<P, D> for QueueReader<P, D> {
    async fn read(&mut self) -> Result<Option<P::Context>, Error> {
        self.values.pop_front().unwrap_or_else(|| Ok(None))
    }
}

pub(super) struct RecordingWriter<P: Protocol, D: Direction> {
    pub(super) writes: Arc<Mutex<Vec<P::Context>>>,
    pub(super) failure: Option<Error>,
    marker: PhantomData<D>,
}

impl<P: Protocol, D: Direction> RecordingWriter<P, D> {
    pub(super) fn new(writes: Arc<Mutex<Vec<P::Context>>>) -> Self {
        Self {
            writes,
            failure: None,
            marker: PhantomData,
        }
    }
}

#[async_trait]
impl<P: Protocol, D: Direction> Writer<P, D> for RecordingWriter<P, D> {
    async fn write(&mut self, context: P::Context) -> Result<P::Context, Error> {
        if let Some(error) = self.failure.clone() {
            return Err(error);
        }
        self.writes.lock().push(context.clone());
        Ok(context)
    }
}

pub(super) fn document(value: &str) -> Document {
    let schema = DocumentSchema::new(
        DocumentSchemaId::new("message").unwrap(),
        1,
        "Message",
        vec![
            DocumentField::new(
                DocumentFieldName::new("value").unwrap(),
                DocumentFieldType::String,
                "Value",
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let mut document = Document::new(schema);
    document
        .set("value", DocumentValue::String(value.to_owned()))
        .unwrap();
    document
}

pub(super) fn text(document: &Document) -> String {
    let DocumentValue::String(value) = document.get("value").unwrap() else {
        panic!("value field must be String");
    };
    value.clone()
}
