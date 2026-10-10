// SPDX-License-Identifier: Apache-2.0
//! Data model for the D-Bus introspection XML format
//! (`org.freedesktop.DBus.Introspectable`), with serde support via `quick-xml`.

use serde::{Deserialize, Serialize};

pub use quick_xml::{DeError, SeError};

/// A `<node>` element: the root of an introspection document, or a child object.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename = "node")]
pub struct Node {
    #[serde(rename = "@name", default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "interface", default)]
    pub interfaces: Vec<Interface>,
    #[serde(rename = "node", default)]
    pub nodes: Vec<Node>,
}

impl Node {
    /// Parses an introspection XML document.
    pub fn from_xml(xml: &str) -> Result<Self, DeError> {
        quick_xml::de::from_str(xml)
    }

    /// Serializes this node back to introspection XML.
    pub fn to_xml(&self) -> Result<String, SeError> {
        quick_xml::se::to_string(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interface {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "method", default)]
    pub methods: Vec<Method>,
    #[serde(rename = "signal", default)]
    pub signals: Vec<Signal>,
    #[serde(rename = "property", default)]
    pub properties: Vec<Property>,
    #[serde(rename = "annotation", default)]
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Method {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "arg", default)]
    pub args: Vec<Arg>,
    #[serde(rename = "annotation", default)]
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signal {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "arg", default)]
    pub args: Vec<Arg>,
    #[serde(rename = "annotation", default)]
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arg {
    #[serde(rename = "@name", default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "@type")]
    pub ty: String,
    /// Defaults to `in` for methods; always `out` (and usually omitted) for signals.
    #[serde(rename = "@direction", default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Direction>,
    #[serde(rename = "annotation", default)]
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    In,
    Out,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Property {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@type")]
    pub ty: String,
    #[serde(rename = "@access")]
    pub access: Access,
    #[serde(rename = "annotation", default)]
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    Read,
    Write,
    ReadWrite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@value")]
    pub value: String,
}
