//! Host operation contract.
//!
//! A host operation may run in the caller process or in a remote `mcpmd serve`, so its input and
//! output must cross a process boundary. Both are JSON values, the message bodies of
//! [`messaging`](crate::messaging): [`HostOperationInput`] and [`HostOperationOutput`] each define
//! the conversion from and to [`Value`] used on that boundary.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// A dispatchable host operation.
///
/// `Input` is produced by the caller and decoded by the process that runs the operation;
/// `Output` travels the other way. Both directions use JSON.
pub trait HostOperation {
    type Input: HostOperationInput;
    type Output: HostOperationOutput;
}

/// Input of a host operation, transported as a JSON value.
pub trait HostOperationInput {
    /// Deserializes `value` into `Self` and erases the concrete type behind the trait object.
    ///
    /// The default implementation decodes with serde for types that derive `Serialize` and
    /// `Deserialize`; implementors only provide [`to_value`](Self::to_value). This method is not
    /// available on the trait object itself because `Self: Sized` is required to build it.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcpm_exec::exec::HostOperationInput;
    /// use serde::{Deserialize, Serialize};
    /// use serde_json::{Value, json};
    ///
    /// #[derive(Serialize, Deserialize)]
    /// struct Input {
    ///     paths: Vec<String>,
    /// }
    ///
    /// impl HostOperationInput for Input {
    ///     fn to_value(&self) -> Result<Value, serde_json::Error> {
    ///         serde_json::to_value(self)
    ///     }
    /// }
    ///
    /// let input: Box<dyn HostOperationInput> = Input::from_value(json!({"paths": ["a.jar"]})).unwrap();
    /// assert_eq!(input.to_value().unwrap(), json!({"paths": ["a.jar"]}));
    /// ```
    fn from_value(value: Value) -> Result<Box<dyn HostOperationInput>, serde_json::Error>
    where
        Self: Sized + Serialize + DeserializeOwned + 'static,
    {
        Ok(Box::new(serde_json::from_value::<Self>(value)?))
    }

    /// Serializes this input to a JSON value.
    ///
    /// Implementations encode with serde, typically `serde_json::to_value(self)`.
    fn to_value(&self) -> Result<Value, serde_json::Error>;
}

/// Output of a host operation, transported as a JSON value.
pub trait HostOperationOutput {
    /// Deserializes `value` into `Self` and erases the concrete type behind the trait object.
    ///
    /// The default implementation decodes with serde for types that derive `Serialize` and
    /// `Deserialize`; implementors only provide [`to_value`](Self::to_value). This method is not
    /// available on the trait object itself because `Self: Sized` is required to build it.
    fn from_value(value: Value) -> Result<Box<dyn HostOperationOutput>, serde_json::Error>
    where
        Self: Sized + Serialize + DeserializeOwned + 'static,
    {
        Ok(Box::new(serde_json::from_value::<Self>(value)?))
    }

    /// Serializes this output to a JSON value.
    ///
    /// Implementations encode with serde, typically `serde_json::to_value(self)`.
    fn to_value(&self) -> Result<Value, serde_json::Error>;
}
