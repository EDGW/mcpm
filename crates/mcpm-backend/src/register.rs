//! Single-truth operation registration.
//!
//! Each operation declares its identifier once through [`host_operation`] or
//! [`surface_operation`]. Host operations additionally declare their JSON codec and submit
//! one registration entry, so the serve registry lists exactly the host operations this
//! build contains; there is no second identifier table anywhere.

use std::fmt;

use mcpm_exec::{DispatchError, Operation, Provide, Registry, RegistryError, Wire};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::Providers;

/// A host operation: its identifier, its JSON codec and its registry entry come from the
/// same declaration.
///
/// The serde bounds cannot be written on [`Operation`] itself, because surface operations
/// never cross a process boundary; they live here instead.
pub trait HostOperation: Operation
where
    Self::Input: Serialize + DeserializeOwned,
    Self::Event: Serialize + DeserializeOwned,
    Self::Output: Serialize + DeserializeOwned,
    Self::Error: Serialize + DeserializeOwned,
{
    /// Stable identifier used by callers and the serve registry.
    const ID: &'static str;

    /// Codec used for inputs, events, outputs and errors on the wire.
    const WIRE: Wire<Self>;
}

/// A surface operation: always dispatched in the calling process and never served.
pub trait SurfaceOperation: Operation {
    /// Stable identifier used by callers.
    const ID: &'static str;
}

/// One host operation ready to be installed into the serve registry.
pub struct HostRegistration {
    id: &'static str,
    install: fn(&mut Registry<Providers>) -> Result<(), RegistryError>,
}

impl HostRegistration {
    /// Builds the entry for one operation; the compiler rejects an operation without a
    /// codec or without a host environment.
    pub const fn new<O>() -> Self
    where
        O: HostOperation,
        O::Input: Serialize + DeserializeOwned,
        O::Event: Serialize + DeserializeOwned,
        O::Output: Serialize + DeserializeOwned,
        O::Error: Serialize + DeserializeOwned,
        Providers: Provide<O::Env>,
    {
        fn install<O>(registry: &mut Registry<Providers>) -> Result<(), RegistryError>
        where
            O: HostOperation,
            O::Input: Serialize + DeserializeOwned,
            O::Event: Serialize + DeserializeOwned,
            O::Output: Serialize + DeserializeOwned,
            O::Error: Serialize + DeserializeOwned,
            Providers: Provide<O::Env>,
        {
            registry.register::<O>()
        }
        Self {
            id: <O as HostOperation>::ID,
            install: install::<O>,
        }
    }

    /// The operation identifier this entry serves.
    pub fn id(&self) -> &'static str {
        self.id
    }

    pub(crate) fn install(&self, registry: &mut Registry<Providers>) -> Result<(), RegistryError> {
        (self.install)(registry)
    }
}

inventory::collect!(HostRegistration);

/// Builds the serve registry from every host operation this build declares.
pub fn register_host_operations(providers: Providers) -> Result<Registry<Providers>, RegistryError> {
    let mut registry = Registry::new(providers);
    for entry in inventory::iter::<HostRegistration> {
        entry.install(&mut registry)?;
    }
    Ok(registry)
}

/// Declares a host operation's identifier, codec and serve registration.
///
/// The operation module implements [`Operation`] with
/// `const ID = <Self as HostOperation>::ID` and `const WIRE = Some(<Self as HostOperation>::WIRE)`,
/// so the identifier and codec have exactly one declaration.
#[macro_export]
macro_rules! host_operation {
    ($operation:ty, $id:literal) => {
        impl $crate::register::HostOperation for $operation {
            const ID: &'static str = $id;
            const WIRE: ::mcpm_exec::Wire<Self> = ::mcpm_exec::Wire::new(
                $crate::register::encode_input::<Self>,
                $crate::register::decode_input::<Self>,
                $crate::register::encode_event::<Self>,
                $crate::register::decode_event::<Self>,
                $crate::register::encode_output::<Self>,
                $crate::register::decode_output::<Self>,
                $crate::register::encode_error::<Self>,
                $crate::register::decode_error::<Self>,
            );
        }

        ::inventory::submit! {
            $crate::register::HostRegistration::new::<$operation>()
        }
    };
}

/// Declares a surface operation's identifier.
#[macro_export]
macro_rules! surface_operation {
    ($operation:ty, $id:literal) => {
        impl $crate::register::SurfaceOperation for $operation {
            const ID: &'static str = $id;
        }
    };
}

pub fn encode_input<O: Operation>(value: O::Input) -> Result<Value, DispatchError>
where
    O::Input: Serialize,
{
    encode(value)
}

pub fn decode_input<O: Operation>(value: Value) -> Result<O::Input, DispatchError>
where
    O::Input: DeserializeOwned,
{
    decode(value)
}

pub fn encode_event<O: Operation>(value: O::Event) -> Result<Value, DispatchError>
where
    O::Event: Serialize,
{
    encode(value)
}

pub fn decode_event<O: Operation>(value: Value) -> Result<O::Event, DispatchError>
where
    O::Event: DeserializeOwned,
{
    decode(value)
}

pub fn encode_output<O: Operation>(value: O::Output) -> Result<Value, DispatchError>
where
    O::Output: Serialize,
{
    encode(value)
}

pub fn decode_output<O: Operation>(value: Value) -> Result<O::Output, DispatchError>
where
    O::Output: DeserializeOwned,
{
    decode(value)
}

pub fn encode_error<O: Operation>(value: O::Error) -> Result<Value, DispatchError>
where
    O::Error: Serialize,
{
    encode(value)
}

pub fn decode_error<O: Operation>(value: Value) -> Result<O::Error, DispatchError>
where
    O::Error: DeserializeOwned,
{
    decode(value)
}

fn encode<T: Serialize>(value: T) -> Result<Value, DispatchError> {
    serde_json::to_value(value).map_err(protocol)
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, DispatchError> {
    serde_json::from_value(value).map_err(protocol)
}

fn protocol(error: serde_json::Error) -> DispatchError {
    DispatchError::Protocol {
        message: error.to_string(),
    }
}

impl fmt::Debug for HostRegistration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostRegistration")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}
