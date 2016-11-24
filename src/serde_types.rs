// Included directly into client.rs after pre-processing by serde.

use chrono::{DateTime, UTC};
use serde::{self, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::marker::PhantomData;
use std::result;
use std::str::FromStr;
use serde_json;

use errors::*;

//-------------------------------------------------------------------------
// ResourceStatus interfaces

/// A BigML status code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceStatusCode {
    /// BigML is waiting on another resource before processing this one.
    Waiting,
    /// The processing job has been added to the queue.
    Queued,
    /// Actual processing has started.
    Started,
    /// Part of the job has been performed.
    InProgress,
    /// Summary statistics for a dataset are available.
    Summarized,
    /// The resource is ready.
    Finished,
    /// Something went wrong processing the task.
    Faulty,
    /// Something has gone wrong in BigML, perhaps an outage.
    Unknown,
}

impl ResourceStatusCode {
    /// Is BigML still working on ingesting and processing this resource?
    pub fn is_working(self) -> bool {
        use ResourceStatusCode::*;
        match self {
            Waiting | Queued | Started | InProgress | Summarized => true,
            _ => false,
        }
    }

    /// Has BigML successfully finished processing this resource?
    pub fn is_ready(self) -> bool {
        self == ResourceStatusCode::Finished
    }

    /// Did something go wrong while processing this resource?
    pub fn is_err(self) -> bool {
        self == ResourceStatusCode::Faulty ||
            self == ResourceStatusCode::Unknown
    }
}

impl Deserialize for ResourceStatusCode {
    fn deserialize<D>(deserializer: &mut D) -> result::Result<Self, D::Error>
        where D: Deserializer
    {
        match i64::deserialize(deserializer)? {
            0 => Ok(ResourceStatusCode::Waiting),
            1 => Ok(ResourceStatusCode::Queued),
            2 => Ok(ResourceStatusCode::Started),
            3 => Ok(ResourceStatusCode::InProgress),
            4 => Ok(ResourceStatusCode::Summarized),
            5 => Ok(ResourceStatusCode::Finished),
            -1 => Ok(ResourceStatusCode::Faulty),
            -2 => Ok(ResourceStatusCode::Unknown),
            code => {
                let msg = format!("Unknown BigML resource status code {}", code);
                Err(<D::Error as serde::Error>::invalid_value(&msg))
            }
        }
    }
}

/// Status of a resource.  BigML actually defines many different "status"
/// types, one for each resource, but quite a few of them have are highly
/// similar.  This interface tries to generalize over the most common
/// versions.
pub trait ResourceStatus {
    /// Status code.
    fn code(&self) -> ResourceStatusCode;

    /// Human-readable status message.
    fn message(&self) -> &str;

    /// Number of milliseconds which were needed to create this resource.
    fn elapsed(&self) -> Option<u64>;

    /// Number between 0.0 and 1.0 representing the progress of creating
    /// this resource.
    fn progress(&self) -> Option<f32>;
}

/// Status of a generic resource.
#[derive(Debug, Deserialize)]
pub struct GenericResourceStatus {
    /// Status code.
    pub code: ResourceStatusCode,

    /// Human-readable status message.
    pub message: String,

    /// Number of milliseconds which were needed to create this resource.
    pub elapsed: Option<u64>,

    /// Number between 0.0 and 1.0 representing the progress of creating
    /// this resource.
    pub progress: Option<f32>,

    /// Having one hidden field makes it possible to extend this struct
    /// without breaking semver API guarantees.
    #[serde(default, skip_serializing)]
    _hidden: (),
}

impl ResourceStatus for GenericResourceStatus {
    fn code(&self) -> ResourceStatusCode {
        self.code
    }

    fn message(&self) -> &str {
        &self.message
    }

    fn elapsed(&self) -> Option<u64> {
        self.elapsed
    }

    fn progress(&self) -> Option<f32> {
        self.progress
    }
}

//-------------------------------------------------------------------------
// ResourceProperties interfaces

/// This trait allows access to common properties shared by all resource
/// types.
pub trait ResourceProperties: fmt::Debug + Deserialize {
    /// The status code for this resource.
    fn status(&self) -> &ResourceStatus;
}

/// A trait representing a BigML data type.  Caution!  This is a very
/// abstract trait and implementations are not expected to carry any actual
/// data.  Rather, this mostly exists to be used as a "tag" and to create
/// associations between related types.
pub trait Resource {
    /// The properties of resources of this type.
    type Properties: ResourceProperties;

    /// The prefix used for all IDs of this type.
    fn id_prefix() -> &'static str;
}

//-------------------------------------------------------------------------
// ResourceId

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

//-------------------------------------------------------------------------
// Model type

/// BigML supports two main kinds of models: "classification" models, which
/// are used to predict category properties, and "regression" models, which
/// are used to predict numeric properties.  These models are treated
/// differently in many places.
///
/// `ModelType` is a "marker" trait that we use to keep track of which kind
/// of model we're working with.  It allows us to figure out which related
/// types are associated with each model.
///
/// We inherit from the trait `fmt::Debug`, not because anybody should ever
/// create or print a `ModelType`, but because we're used as type parameter
/// to many structs which `#[derive(Debug)]`, which won't work unless all
/// type parameters are themselves `fmt::Debug`, even if they're not needed
/// to print the struct.
///
/// TODO: Remove `Deserialize` trait once we finish collapsing types a bit.
pub trait ModelType: fmt::Debug + Deserialize {
    /// The results of an evaluation of this model.
    type EvaluationResult: fmt::Debug + Deserialize + Serialize + 'static;
}

/// Classification models are used to predict category properties.
#[derive(Debug, Deserialize)]
pub struct ClassificationModel;

impl ModelType for ClassificationModel {
    type EvaluationResult = ClassificationEvaluationResult;
}

// TODO: RegressionModel and RegressionEvaluationResult.

//-------------------------------------------------------------------------
// Resource definition tools

macro_rules! resource {
    (
        name $name:ident, $string_name:expr;

        $(#[ $property_type_meta:meta ])*
        pub struct $property_type:ident $(<$($Ty:ident : $Tr:ident),*>)* {
            $(
                $(#[ $field_type_meta:meta ])*
                pub $field_name:ident: $field_ty:ty,
            )*
        }

    ) => {
        /// Tag type (never instantiated) used to identify resources of
        /// this type throughout the API.
        pub struct $name $(<$($Ty : $Tr),*>)* {
            $(
                $(_hidden: PhantomData<$Ty>)*,
            )*
        }

        impl $(<$($Ty : $Tr),*>)* Resource for $name $(<$($Ty),*>)* {
            type Properties = $property_type $(<$($Ty),*>)*;

            fn id_prefix() -> &'static str {
                concat!($string_name, "/")
            }
        }

        $(#[ $property_type_meta ])*
        pub struct $property_type $(<$($Ty : $Tr),*>)* {
            // Start by declaring the fields which appear on every resource
            // type.  We should theoretically implement this using
            // inheritance, but Rust doesn't have implementation
            // inheritance.  We could also implement this using various
            // other Rust patterns like delegation, but that would mean
            // that serde could no longer assume a simple 1-to-1 mapping
            // between Rust and JSON types. So we just use a macro to do
            // some code gen, and we define a `ResourceProperties` trait
            // that we can use to access any duplicated bits using a single
            // API.

            /// Used to classify by industry or category.  0 is "Miscellaneous".
            pub category: i64,

            /// An HTTP status code, typically either 201 or 200.
            ///
            /// TODO: Deserialize as a `reqwest::StatusCode`?
            pub code: u16,

            /// The time this resource was created.
            pub created: DateTime<UTC>,

            /// Was this created in development mode?
            pub dev: bool,

            /// Text describing this resource.  May contain limited Markdown.
            pub description: String,

            /// The name of this resource
            pub name: String,

            // What project is this associated with?
            //
            // TODO: Define `Project` type and then enable this.
            //pub project: ResourceId<Project>,

            /// Has this been shared using a private link?
            pub shared: bool,

            /// Was this created using a subscription plan?
            pub subscription: bool,

            /// User-defined tags.
            pub tags: Vec<String>,

            /// The last time this was updated.
            pub updated: DateTime<UTC>,

            /// The ID of this execution.
            pub resource: ResourceId<$name $(<$($Ty),*>)*>,

            /// Having one hidden field makes it possible to extend this struct
            /// without breaking semver API guarantees.
             #[serde(default, skip_serializing)]
            _hidden: (),

            $(
                $(#[ $field_type_meta ])*
                pub $field_name: $field_ty
            ),*
        }

        impl $(<$($Ty : $Tr),*>)* ResourceProperties for $property_type $(<$($Ty),*>)* {
            fn status(&self) -> &ResourceStatus {
                &self.status
            }
        }
    };
}

//-------------------------------------------------------------------------
// Ensemble

// An ensemble of multiple predictive models.
resource! {
    name Ensemble, "ensemble";

    /// Properties of an ensemble resource.
    ///
    /// TODO: Still lots of missing fields.
    #[derive(Debug, Deserialize)]
    pub struct EnsembleProperties {
        /// The current status of this ensemble.
        pub status: GenericResourceStatus,

        // The dataset used to create this ensemble.
        //pub dataset: ResourceId<Dataset>,
    }
}

//-------------------------------------------------------------------------
// Evaluation

resource! {
    name Evaluation, "evaluation";

    /// Properties of a BigML evaluation.
    ///
    /// TODO: Still lots of missing fields.
    #[derive(Debug, Deserialize)]
    pub struct EvaluationProperties<M: ModelType> {
        /// The status of this resource.
        pub status: GenericResourceStatus,

        /// The result of this evaluation.
        pub result: M::EvaluationResult,
    }
}

/// The result of an evaluation.
#[derive(Debug, Deserialize, Serialize)]
pub struct ClassificationEvaluationResult {
    /// The names of our classifications.
    pub class_names: Vec<String>,

    /// According to BigML, "Measures the performance of the classifier
    /// that predicts the mode class for all the instances in the dataset."
    pub mode: DetailedClassificationEvaluationResult,

    /// The performance of this model.
    pub model: DetailedClassificationEvaluationResult,

    /// According to BigML, "Measures the performance of the classifier
    /// that predicts a random class for all the instances in the dataset."
    pub random: DetailedClassificationEvaluationResult,

    /// Having one hidden field makes it possible to extend this struct
    /// without breaking semver API guarantees.
    #[serde(default, skip_serializing)]
    _hidden: (),
}

/// The detailed result of an evaluation using specific criteria.
#[derive(Debug, Deserialize, Serialize)]
pub struct DetailedClassificationEvaluationResult {
    /// The portion of instances we classified correctly.
    pub accuracy: f64,
    /// The average f-measure over all classes.
    pub average_f_measure: f64,
    /// The average phi over all classes.
    pub average_phi: f64,
    /// The average precision over all classes.
    pub average_precision: f64,
    /// The average recall over all classes.
    pub average_recall: f64,
    /// A list of rows of the confusion matrix for this model.
    pub confusion_matrix: Vec<Vec<f64>>,
    /// Statistics for each of the individidual classes.
    pub per_class_statistics: Vec<ClassificationPerClassStatistics>,
    /// Having one hidden field makes it possible to extend this struct
    /// without breaking semver API guarantees.
    #[serde(default, skip_serializing)]
    _hidden: (),
}

/// The detailed result of an evaluation using specific criteria.
#[derive(Debug, Deserialize, Serialize)]
pub struct ClassificationPerClassStatistics {
    /// The portion of instances in this class that were correctly
    /// classified.
    pub accuracy: f64,
    /// The the of this class.
    pub class_name: String,
    /// The harmonic mean of precision and recall.
    pub f_measure: f64,
    /// See
    /// [Wikipedia](http://en.wikipedia.org/wiki/Matthews_correlation_coefficient).
    pub phi_coefficient: f64,
    /// The fraction of positives that were true positives. (TP / (TP + FP))
    pub precision: f64,
    /// The number of true positives over the number of actual positives in
    /// the dataset. (TP / (TP + FN))
    pub recall: f64,
}

//-------------------------------------------------------------------------
// Executions

// An execution of a WhizzML script.
resource! {
    name Execution, "execution";

    /// Properties of a BigML execution.
    ///
    /// TODO: Still lots of missing fields.
    #[derive(Debug, Deserialize)]
    pub struct ExecutionProperties {
        /// The current status of this execution.
        pub status: GenericResourceStatus,

        /// Further information about this execution.
        pub execution: ExecutionData,
    }
}

/// Data about a script execution.
///
/// TODO: Lots of missing fields.
#[derive(Debug, Deserialize)]
pub struct ExecutionData {
    /// Names, values and types of resources output by the script.
    outputs: Vec<(String, serde_json::Value, String)>,

    /// Result values from the script.
    result: Vec<serde_json::Value>,

    /// Having one hidden field makes it possible to extend this struct
    /// without breaking semver API guarantees.
    #[serde(default, skip_serializing)]
    _hidden: (),
}

//-------------------------------------------------------------------------
// Sources

// A data source used by BigML.
resource! {
    name Source, "source";

    /// Properties of BigML source.
    ///
    /// TODO: Still lots of missing fields.
    #[derive(Debug, Deserialize)]
    pub struct SourceProperties {
        /// The status of this source.
        pub status: GenericResourceStatus,

        /// The name of the file uploaded.
        pub file_name: String,

        /// An MD5 hash of the uploaded file.
        pub md5: String,

        /// The number of bytes of the source.
        pub size: u64,
    }
}
