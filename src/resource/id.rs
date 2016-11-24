//! Resource identifiers used by the BigML API.

use serde::{self, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::marker::PhantomData;
use std::result;
use std::str::FromStr;

use errors::*;
use super::Resource;

/// A strongly-typed "resource ID" used to identify many different kinds of
/// BigML resources.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResourceId<R: Resource> {
    /// The ID of the resource.
    id: String,
    /// A special 0-byte field which exists just to mention the type `R`
    /// inside the struct, and thus avoid compiler errors about unused type
    /// parameters.
    _phantom: PhantomData<R>,
}

impl<R: Resource> ResourceId<R> {
    /// Get this resource as a string.
    pub fn as_str(&self) -> &str {
        &self.id
    }
}

impl<R: Resource> FromStr for ResourceId<R> {
    type Err = Error;

    fn from_str(id: &str) -> Result<Self> {
        if !id.starts_with(R::id_prefix()) {
            Ok(ResourceId {
                id: id.to_owned(),
                _phantom: PhantomData,
            })
        } else {
            Err(ErrorKind::WrongResourceType(R::id_prefix(), id.to_owned()).into())
        }
    }
}

impl<R: Resource> fmt::Debug for ResourceId<R> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}", &self.id)
    }
}

impl<R: Resource> fmt::Display for ResourceId<R> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}", &self.id)
    }
}

impl<R: Resource> Deserialize for ResourceId<R> {
    fn deserialize<D>(deserializer: &mut D) -> result::Result<Self, D::Error>
        where D: Deserializer
    {
        let id: String = String::deserialize(deserializer)?;
        if !id.starts_with(R::id_prefix()) {
            Ok(ResourceId {
                id: id,
                _phantom: PhantomData,
            })
        } else {
            let err: Error =
                ErrorKind::WrongResourceType(R::id_prefix(), id).into();
            Err(<D::Error as serde::Error>::invalid_value(&format!("{}", err)))
        }
    }
}

impl<R: Resource> Serialize for ResourceId<R> {
    fn serialize<S>(&self, serializer: &mut S) -> result::Result<(), S::Error>
        where S: Serializer
    {
        self.id.serialize(serializer)
    }
}
