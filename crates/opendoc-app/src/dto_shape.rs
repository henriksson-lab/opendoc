//! A serde-driven description of what a DTO actually puts on the wire.
//!
//! The TypeScript DTO literals in
//! `opendoc-api/src/bin/generate_command_contract.rs` are hand-written strings
//! that mirror Rust structs. Nothing in the generator can see the structs —
//! `opendoc-api` cannot depend on `opendoc-app`, which is why the literals are
//! written out by hand in the first place — so `generate:commands --check`
//! compares the generator's output against itself and is blind to a Rust
//! struct that grew, lost or renamed a field.
//!
//! This module closes that hole from the other side. It derives the wire shape
//! of a type from its own `Serialize`/`Deserialize` impls, with no sample value
//! written by hand anywhere:
//!
//! * [`struct_fields`] runs a `Deserializer` that answers nothing and instead
//!   reports the `FIELDS` list serde's derive hands it — the authoritative,
//!   post-`rename` field names.
//! * [`build`] is a `Deserializer` that constructs a value out of thin air, in
//!   two modes: *minimal* (every `Option` `None`, every collection empty) and
//!   *filled* (every `Option` `Some`, one element per collection). Serializing
//!   both says exactly which fields `skip_serializing_if` can drop and which
//!   ones can arrive as `null`.
//! * [`enum_variants`] does the same trick for the unit enums the contract
//!   projects as TypeScript string unions.
//!
//! Because nothing here is a hand-written sample, the description cannot rot:
//! it is whatever the type does today.

use serde::de::{
    self, DeserializeSeed, Deserializer, EnumAccess, IntoDeserializer, MapAccess, SeqAccess,
    VariantAccess, Visitor,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// How deep [`build`] keeps filling before it starts answering `None`/empty.
///
/// A bound is required, not a nicety: `AppBlock` contains a table of
/// `AppBlock`s, so a filled value of it is infinite without one. Five is deep
/// enough that every direct field of a top-level DTO, and the fields of the
/// structs it directly contains, are populated.
const MAX_FILL_DEPTH: u32 = 5;

#[derive(Debug)]
pub(crate) enum ProbeError {
    /// `deserialize_struct` was reached: the payload is the struct's own name
    /// and serde's `FIELDS`.
    Struct(&'static str, Vec<String>),
    /// `deserialize_enum` was reached: the name and serde's `VARIANTS`.
    Enum(&'static str, Vec<String>),
    Message(String),
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProbeError::Struct(name, fields) => {
                write!(f, "struct {name} with fields {fields:?}")
            }
            ProbeError::Enum(name, variants) => {
                write!(f, "enum {name} with variants {variants:?}")
            }
            ProbeError::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ProbeError {}

impl de::Error for ProbeError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        ProbeError::Message(msg.to_string())
    }
}

/// A `Deserializer` that refuses every request and reports what was asked of
/// it. Used to read serde's own `FIELDS`/`VARIANTS` off a derived impl.
struct Probe;

macro_rules! probe_refuses {
    ($($method:ident),* $(,)?) => {
        $(
            fn $method<V>(self, _visitor: V) -> Result<V::Value, ProbeError>
            where
                V: Visitor<'de>,
            {
                Err(ProbeError::Message(format!(
                    "expected a struct or enum, serde asked for {}",
                    stringify!($method)
                )))
            }
        )*
    };
}

impl<'de> Deserializer<'de> for Probe {
    type Error = ProbeError;

    probe_refuses!(
        deserialize_any,
        deserialize_bool,
        deserialize_i8,
        deserialize_i16,
        deserialize_i32,
        deserialize_i64,
        deserialize_u8,
        deserialize_u16,
        deserialize_u32,
        deserialize_u64,
        deserialize_f32,
        deserialize_f64,
        deserialize_char,
        deserialize_str,
        deserialize_string,
        deserialize_bytes,
        deserialize_byte_buf,
        deserialize_option,
        deserialize_unit,
        deserialize_seq,
        deserialize_map,
        deserialize_identifier,
        deserialize_ignored_any,
    );

    fn deserialize_unit_struct<V>(
        self,
        name: &'static str,
        _visitor: V,
    ) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        Err(ProbeError::Struct(name, Vec::new()))
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_tuple<V>(self, _len: usize, _visitor: V) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        Err(ProbeError::Message(
            "expected a struct or enum, serde asked for a tuple".to_string(),
        ))
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        _visitor: V,
    ) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        Err(ProbeError::Message(
            "expected a struct or enum, serde asked for a tuple struct".to_string(),
        ))
    }

    fn deserialize_struct<V>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        Err(ProbeError::Struct(
            name,
            fields.iter().map(|field| (*field).to_string()).collect(),
        ))
    }

    fn deserialize_enum<V>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        Err(ProbeError::Enum(
            name,
            variants
                .iter()
                .map(|variant| (*variant).to_string())
                .collect(),
        ))
    }
}

/// The serde field names of `T`, exactly as serde's derive declares them.
pub(crate) fn struct_fields<T>() -> Result<Vec<String>, String>
where
    T: for<'de> Deserialize<'de>,
{
    match T::deserialize(Probe) {
        Err(ProbeError::Struct(_, fields)) => Ok(fields),
        Err(other) => Err(format!("not a plain struct: {other}")),
        Ok(_) => Err("probe deserializer unexpectedly produced a value".to_string()),
    }
}

/// The serde variant names of `T`, exactly as serde's derive declares them.
pub(crate) fn enum_variants<T>() -> Result<Vec<String>, String>
where
    T: for<'de> Deserialize<'de>,
{
    match T::deserialize(Probe) {
        Err(ProbeError::Enum(_, variants)) => Ok(variants),
        Err(other) => Err(format!("not a plain enum: {other}")),
        Ok(_) => Err("probe deserializer unexpectedly produced a value".to_string()),
    }
}

/// A `Deserializer` that invents a value of whatever shape it is asked for.
#[derive(Clone, Copy)]
struct Build {
    fill: bool,
    depth: u32,
}

impl Build {
    fn filling(self) -> bool {
        self.fill && self.depth < MAX_FILL_DEPTH
    }

    fn deeper(self) -> Self {
        Self {
            fill: self.fill,
            depth: self.depth + 1,
        }
    }

    fn count(self) -> usize {
        usize::from(self.filling())
    }
}

macro_rules! build_number {
    ($($method:ident => $visit:ident : $ty:ty),* $(,)?) => {
        $(
            fn $method<V>(self, visitor: V) -> Result<V::Value, ProbeError>
            where
                V: Visitor<'de>,
            {
                visitor.$visit(if self.filling() { 1 as $ty } else { 0 as $ty })
            }
        )*
    };
}

impl<'de> Deserializer<'de> for Build {
    type Error = ProbeError;

    /// Reached only by self-describing types (`serde_json::Value`). Answering
    /// `null` keeps them constructible without inventing a shape for them.
    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }

    build_number!(
        deserialize_i8 => visit_i8: i8,
        deserialize_i16 => visit_i16: i16,
        deserialize_i32 => visit_i32: i32,
        deserialize_i64 => visit_i64: i64,
        deserialize_u8 => visit_u8: u8,
        deserialize_u16 => visit_u16: u16,
        deserialize_u32 => visit_u32: u32,
        deserialize_u64 => visit_u64: u64,
        deserialize_f32 => visit_f32: f32,
        deserialize_f64 => visit_f64: f64,
    );

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_bool(self.filling())
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_char('x')
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_str(if self.filling() { "x" } else { "" })
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_byte_buf(if self.filling() { vec![1] } else { Vec::new() })
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        if self.filling() {
            visitor.visit_some(self.deeper())
        } else {
            visitor.visit_none()
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_seq(BuildSeq {
            remaining: self.count(),
            build: self.deeper(),
        })
    }

    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_seq(BuildSeq {
            remaining: len,
            build: self.deeper(),
        })
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_map(BuildMap {
            remaining: self.count(),
            build: self.deeper(),
        })
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_map(BuildStruct {
            fields,
            index: 0,
            build: self.deeper(),
        })
    }

    fn deserialize_enum<V>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        let Some(variant) = variants.first() else {
            return Err(ProbeError::Message(format!("enum {name} has no variants")));
        };
        visitor.visit_enum(BuildEnum {
            variant,
            build: self.deeper(),
        })
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }
}

struct BuildSeq {
    remaining: usize,
    build: Build,
}

impl<'de> SeqAccess<'de> for BuildSeq {
    type Error = ProbeError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, ProbeError>
    where
        T: DeserializeSeed<'de>,
    {
        if self.remaining == 0 {
            return Ok(None);
        }
        self.remaining -= 1;
        seed.deserialize(self.build).map(Some)
    }
}

struct BuildMap {
    remaining: usize,
    build: Build,
}

impl<'de> MapAccess<'de> for BuildMap {
    type Error = ProbeError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, ProbeError>
    where
        K: DeserializeSeed<'de>,
    {
        if self.remaining == 0 {
            return Ok(None);
        }
        self.remaining -= 1;
        // A map key is filled even in minimal mode: an empty-string key is a
        // legal `BTreeMap<String, _>` key, but a `None` one is not a key at all.
        seed.deserialize(Build {
            fill: true,
            depth: self.build.depth,
        })
        .map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, ProbeError>
    where
        V: DeserializeSeed<'de>,
    {
        seed.deserialize(self.build)
    }
}

struct BuildStruct {
    fields: &'static [&'static str],
    index: usize,
    build: Build,
}

impl<'de> MapAccess<'de> for BuildStruct {
    type Error = ProbeError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, ProbeError>
    where
        K: DeserializeSeed<'de>,
    {
        let Some(field) = self.fields.get(self.index) else {
            return Ok(None);
        };
        self.index += 1;
        let key: de::value::StrDeserializer<'_, ProbeError> = (*field).into_deserializer();
        seed.deserialize(key).map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, ProbeError>
    where
        V: DeserializeSeed<'de>,
    {
        seed.deserialize(self.build)
    }
}

struct BuildEnum {
    variant: &'static str,
    build: Build,
}

impl<'de> EnumAccess<'de> for BuildEnum {
    type Error = ProbeError;
    type Variant = Self;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self), ProbeError>
    where
        V: DeserializeSeed<'de>,
    {
        let name: de::value::StrDeserializer<'_, ProbeError> = self.variant.into_deserializer();
        let value = seed.deserialize(name)?;
        Ok((value, self))
    }
}

impl<'de> VariantAccess<'de> for BuildEnum {
    type Error = ProbeError;

    fn unit_variant(self) -> Result<(), ProbeError> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, ProbeError>
    where
        T: DeserializeSeed<'de>,
    {
        seed.deserialize(self.build)
    }

    fn tuple_variant<V>(self, len: usize, visitor: V) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_seq(BuildSeq {
            remaining: len,
            build: self.build.deeper(),
        })
    }

    fn struct_variant<V>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ProbeError>
    where
        V: Visitor<'de>,
    {
        visitor.visit_map(BuildStruct {
            fields,
            index: 0,
            build: self.build.deeper(),
        })
    }
}

/// Builds a value of `T`: `fill` chooses between "every option `Some`, one
/// element per collection" and "every option `None`, every collection empty".
pub(crate) fn build<T>(fill: bool) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    T::deserialize(Build { fill, depth: 0 }).map_err(|error| {
        format!(
            "could not build a {} value of {}: {error}",
            if fill { "filled" } else { "minimal" },
            std::any::type_name::<T>()
        )
    })
}

/// What one serde field does on the wire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WireField {
    /// The key is absent from the JSON object when the field holds its empty
    /// value — i.e. serde is told to skip it. TypeScript must mark it `?`.
    pub(crate) omissible: bool,
    /// The key is present as `null` when the field holds its empty value.
    /// TypeScript must admit `null`.
    pub(crate) nullable: bool,
}

/// What a whole DTO does on the wire.
#[derive(Clone, Debug)]
pub(crate) struct WireShape {
    pub(crate) fields: BTreeMap<String, WireField>,
    /// Anything the shape itself is inconsistent about, reported rather than
    /// swallowed: a field serde declares but never serializes, for instance.
    pub(crate) anomalies: Vec<String>,
}

/// Derives the wire shape of `T` from its own serde impls.
pub(crate) fn wire_shape<T>() -> Result<WireShape, String>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    let declared = struct_fields::<T>()?;
    let minimal = build::<T>(false)?;
    let filled = build::<T>(true)?;
    let minimal = json_object(&minimal)?;
    let filled = json_object(&filled)?;

    let mut anomalies = Vec::new();
    for field in &declared {
        if !filled.contains_key(field) && !minimal.contains_key(field) {
            anomalies.push(format!(
                "serde declares field `{field}` but it never serializes; \
                 the TypeScript literal cannot describe it"
            ));
        }
    }
    let mut fields = BTreeMap::new();
    for (name, _) in filled.iter() {
        if !declared.iter().any(|field| field == name) {
            anomalies.push(format!(
                "field `{name}` serializes but serde's derive does not declare it \
                 (a serialize/deserialize rename mismatch)"
            ));
        }
        fields.insert(
            name.clone(),
            WireField {
                omissible: !minimal.contains_key(name),
                nullable: minimal.get(name) == Some(&serde_json::Value::Null),
            },
        );
    }
    for (name, value) in minimal.iter() {
        if fields.contains_key(name) {
            continue;
        }
        anomalies.push(format!(
            "field `{name}` serializes only when empty; that is not a shape \
             TypeScript can describe"
        ));
        fields.insert(
            name.clone(),
            WireField {
                omissible: false,
                nullable: value.is_null(),
            },
        );
    }
    Ok(WireShape { fields, anomalies })
}

/// Derives the shape a *command argument* DTO must have: one Rust never
/// serializes, so the only contract is what it accepts on the way in.
///
/// `omissible` here means serde deserializes the value with the key absent
/// (`#[serde(default)]`), and `nullable` means it deserializes the key as
/// `null`. Both are probed by doctoring a filled value's JSON and handing it
/// back to serde, so neither can be asserted from a stale reading of the
/// attributes.
pub(crate) fn input_shape<T>() -> Result<WireShape, String>
where
    T: Serialize + serde::de::DeserializeOwned,
{
    let declared = struct_fields::<T>()?;
    let filled = json_object(&build::<T>(true)?)?;
    let mut anomalies = Vec::new();
    if serde_json::from_value::<T>(serde_json::Value::Object(filled.clone())).is_err() {
        anomalies.push(
            "a filled value of this type does not deserialize from its own JSON, so what it              accepts cannot be probed"
                .to_string(),
        );
    }
    for field in &declared {
        if !filled.contains_key(field) {
            anomalies.push(format!(
                "serde declares field `{field}` but a filled value does not serialize it"
            ));
        }
    }
    let mut fields = BTreeMap::new();
    for name in filled.keys() {
        let mut without = filled.clone();
        without.remove(name);
        let omissible = serde_json::from_value::<T>(serde_json::Value::Object(without)).is_ok();
        let mut nulled = filled.clone();
        nulled.insert(name.clone(), serde_json::Value::Null);
        let nullable = serde_json::from_value::<T>(serde_json::Value::Object(nulled)).is_ok();
        fields.insert(
            name.clone(),
            WireField {
                omissible,
                nullable,
            },
        );
    }
    Ok(WireShape { fields, anomalies })
}

fn json_object<T: Serialize>(
    value: &T,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::Object(map)) => Ok(map),
        Ok(other) => Err(format!(
            "{} does not serialize to a JSON object (got {other})",
            std::any::type_name::<T>()
        )),
        Err(error) => Err(format!(
            "{} failed to serialize: {error}",
            std::any::type_name::<T>()
        )),
    }
}
