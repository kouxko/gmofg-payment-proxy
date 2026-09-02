//! Primitive tracing fields retained before typed Exchange observation parsing.

use std::collections::BTreeMap;

use tracing::field::Visit;

#[derive(Clone, Debug)]
enum Primitive {
    Bool(bool),
    I64(i64),
    U64(u64),
    I128(i128),
    U128(u128),
    F64(f64),
    Str(String),
}

impl Primitive {
    fn logical_bytes(&self) -> usize {
        match self {
            Self::Bool(_) => std::mem::size_of::<bool>(),
            Self::I64(_) | Self::U64(_) | Self::F64(_) => std::mem::size_of::<u64>(),
            Self::I128(_) | Self::U128(_) => std::mem::size_of::<u128>(),
            Self::Str(value) => value.len(),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Fields {
    values: BTreeMap<String, Primitive>,
    max_bytes: usize,
    overflowed: bool,
}

impl Fields {
    pub(super) fn new(max_bytes: usize) -> Self {
        Self {
            values: BTreeMap::new(),
            max_bytes: max_bytes.max(1),
            overflowed: false,
        }
    }

    pub(super) fn text(&self, name: &str) -> Option<String> {
        match self.values.get(name)? {
            Primitive::Str(value) => Some(value.clone()),
            Primitive::I64(value) => Some(value.to_string()),
            Primitive::U64(value) => Some(value.to_string()),
            Primitive::I128(value) => Some(value.to_string()),
            Primitive::U128(value) => Some(value.to_string()),
            Primitive::Bool(value) => Some(value.to_string()),
            Primitive::F64(value) => Some(value.to_string()),
        }
    }

    pub(super) fn merge(&mut self, other: &Self) {
        if other.overflowed {
            self.overflowed = true;
            return;
        }
        for (name, value) in &other.values {
            self.insert(name, value.clone());
            if self.overflowed {
                return;
            }
        }
    }

    pub(super) fn logical_bytes(&self) -> usize {
        self.values.iter().fold(0, |total, (name, value)| {
            total
                .saturating_add(name.len())
                .saturating_add(value.logical_bytes())
                .saturating_add(16)
        })
    }

    pub(super) const fn overflowed(&self) -> bool {
        self.overflowed
    }

    fn insert(&mut self, name: &str, value: Primitive) {
        let previous = self
            .values
            .get(name)
            .map_or(0, |item| name.len() + item.logical_bytes() + 16);
        let replacement = name.len() + value.logical_bytes() + 16;
        let next = self
            .logical_bytes()
            .saturating_sub(previous)
            .saturating_add(replacement);
        if next > self.max_bytes {
            self.overflowed = true;
            return;
        }
        self.values.insert(name.to_owned(), value);
    }
}

impl Visit for Fields {
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.insert(field.name(), Primitive::Bool(value));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.insert(field.name(), Primitive::I64(value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.insert(field.name(), Primitive::U64(value));
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.insert(field.name(), Primitive::F64(value));
    }

    fn record_i128(&mut self, field: &tracing::field::Field, value: i128) {
        self.insert(field.name(), Primitive::I128(value));
    }

    fn record_u128(&mut self, field: &tracing::field::Field, value: u128) {
        self.insert(field.name(), Primitive::U128(value));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        // Admission happens before cloning the string, so a large HTTP body, Display, JSON, or HEX
        // field cannot allocate another complete payload on the transaction thread.
        let replacement = field
            .name()
            .len()
            .saturating_add(value.len())
            .saturating_add(16);
        let previous = self
            .values
            .get(field.name())
            .map_or(0, |item| field.name().len() + item.logical_bytes() + 16);
        if self
            .logical_bytes()
            .saturating_sub(previous)
            .saturating_add(replacement)
            > self.max_bytes
        {
            self.overflowed = true;
            return;
        }
        self.values
            .insert(field.name().to_owned(), Primitive::Str(value.to_owned()));
    }

    fn record_debug(&mut self, _field: &tracing::field::Field, _value: &dyn std::fmt::Debug) {
        // Debug output is formatted evidence, not a reversible typed field.
    }
}
