//! A data source used by BigML.

use std::collections::HashMap;

use super::Resource;
use super::id::*;
use super::status::*;

resource! {
    api_name "source";

    /// A data source used by BigML.
    ///
    /// TODO: Still lots of missing fields.
    #[derive(Debug, Deserialize, Clone)]
    pub struct Source {
        /// The status of this source.
        pub status: GenericStatus,

        /// The name of the file uploaded.
        pub file_name: String,

        /// An MD5 hash of the uploaded file.
        pub md5: String,

        /// The number of bytes of the source.
        pub size: u64,

        /// The fields in this source, keyed by BigML internal ID.
        pub fields: HashMap<String, Field>,
    }
}

/// Information about a field in a data source.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Field {
    /// The name of this field.
    pub name: String,
    /// The type of data stored in this field.
    pub optype: Optype,
    /// The locale of this field.
    pub locale: String,
    /// (This is not well-documented in the BigML API.)
    pub missing_tokens: Vec<String>,
}

/// The type of a data field.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum Optype {
    /// Treat this as a numeric value.
    #[serde(rename="numeric")]
    Numeric,
    /// Threat this as a category with multiple possible values, but not
    /// arbitrary strings.
    #[serde(rename="categorical")]
    Categorical,
    /// Treat this as text.  This uses different machine learning
    /// algorithms than `Categorical`.
    #[serde(rename="text")]
    Text,
    /// Treat this as a list of muliple items separated by an auto-detected
    /// separator.
    #[serde(rename="items")]
    Items,
}
