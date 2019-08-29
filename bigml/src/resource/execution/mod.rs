//! An execution of a WhizzML script.

use serde::de;
use serde::de::DeserializeOwned;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json;
use std::fmt;
use std::result;
use url::Url;

use super::id::*;
use super::status::*;
use super::{Library, Script};
use super::{Resource, ResourceCommon};
use crate::client::Client;
use crate::errors::*;

mod args;
mod execution_status;

pub use self::args::*;
pub use self::execution_status::*;

/// An execution of a WhizzML script.
///
/// TODO: Still lots of missing fields.
#[derive(Clone, Debug, Deserialize, Resource, Serialize)]
#[api_name = "execution"]
pub struct Execution {
    /// Common resource information. These fields will be serialized at the
    /// top-level of this structure by `serde`.
    #[serde(flatten)]
    pub common: ResourceCommon,

    /// The ID of this resource.
    pub resource: Id<Execution>,

    /// The current status of this execution.
    pub status: ExecutionStatus,

    /// Further information about this execution.
    pub execution: Data,

    /// Placeholder to allow extensibility without breaking the API.
    #[serde(skip)]
    _placeholder: (),
}

/// Data about a script execution.
///
/// TODO: Lots of missing fields.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Data {
    /// Outputs from this script.
    #[serde(default)]
    pub outputs: Vec<Output>,

    /// Result values from the script.  This is literally whatever value is
    /// returned at the end of the WhizzML script.
    pub result: Option<serde_json::Value>,

    /// Log entries generated by the script.
    #[serde(default)]
    pub logs: Vec<LogEntry>,

    /// BigML resources created by the script.
    #[serde(default)]
    pub output_resources: Vec<OutputResource>,

    /// Source files used as inputs to this execution.
    #[serde(default)]
    pub sources: Vec<Source>,

    /// Placeholder to allow extensibility without breaking the API.
    #[serde(skip)]
    _placeholder: (),
}

impl Data {
    /// Get a named output of this execution.
    pub fn get<D: DeserializeOwned>(&self, name: &str) -> Result<D> {
        for output in &self.outputs {
            if output.name == name {
                return output.get();
            }
        }
        Err(Error::could_not_get_output(name, format_err!("not found")))
    }
}

/// Information about a source code resource.
#[derive(Clone, Debug)]
pub struct Source {
    /// The script or library associated with this source.
    pub id: SourceId,
    /// The description associated with this source.
    pub description: String,
    /// Placeholder to allow extensibility without breaking the API.
    _placeholder: (),
}

impl<'de> Deserialize<'de> for Source {
    fn deserialize<D>(deserializer: D) -> result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        // Do a whole bunch of annoying work needed to deserialize mixed-type
        // arrays.
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Source;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a list with a source ID and a description")
            }

            fn visit_seq<V>(
                self,
                mut visitor: V,
            ) -> result::Result<Self::Value, V::Error>
            where
                V: de::SeqAccess<'de>,
            {
                use serde::de::Error;

                let id = visitor
                    .next_element()?
                    .ok_or_else(|| V::Error::custom("no id field in source"))?;
                let description = visitor.next_element()?.ok_or_else(|| {
                    V::Error::custom("no description field in source")
                })?;

                Ok(Source {
                    id,
                    description,
                    _placeholder: (),
                })
            }
        }

        deserializer.deserialize_seq(Visitor)
    }
}

impl Serialize for Source {
    fn serialize<S>(&self, serializer: S) -> result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(2))?;
        seq.serialize_element(&self.id)?;
        seq.serialize_element(&self.description)?;
        seq.end()
    }
}

/// Either a script or library ID.
#[derive(Clone, Debug)]
pub enum SourceId {
    /// A library ID.
    Library(Id<Library>),
    /// A script ID.
    Script(Id<Script>),
}

impl SourceId {
    /// Build a URL pointing to the BigML dashboard view for this script.
    pub fn dashboard_url(&self) -> Url {
        match self {
            SourceId::Library(id) => id.dashboard_url(),
            SourceId::Script(id) => id.dashboard_url(),
        }
    }

    /// Download the corresponding source code.
    pub async fn fetch_source_code(&self, client: &Client) -> Result<String> {
        match *self {
            SourceId::Library(ref id) => Ok(client.fetch(id).await?.source_code),
            SourceId::Script(ref id) => Ok(client.fetch(id).await?.source_code),
        }
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            SourceId::Library(ref id) => id.fmt(fmt),
            SourceId::Script(ref id) => id.fmt(fmt),
        }
    }
}

impl<'de> Deserialize<'de> for SourceId {
    fn deserialize<D>(deserializer: D) -> result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        // Do a whole bunch of annoying work needed to deserialize mixed-type
        // arrays.
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = SourceId;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a script or library ID")
            }

            fn visit_str<E>(self, value: &str) -> result::Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value.starts_with(Library::id_prefix()) {
                    let id = value
                        .parse()
                        .map_err(|e| de::Error::custom(format!("{}", e)))?;
                    Ok(SourceId::Library(id))
                } else if value.starts_with(Script::id_prefix()) {
                    let id = value
                        .parse()
                        .map_err(|e| de::Error::custom(format!("{}", e)))?;
                    Ok(SourceId::Script(id))
                } else {
                    Err(de::Error::custom("expected script or library ID"))
                }
            }
        }

        deserializer.deserialize_str(Visitor)
    }
}

impl Serialize for SourceId {
    fn serialize<S>(&self, serializer: S) -> result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match *self {
            SourceId::Library(ref id) => id.serialize(serializer),
            SourceId::Script(ref id) => id.serialize(serializer),
        }
    }
}