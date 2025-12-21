pub mod registry;
pub mod routes;
pub mod service;

pub use registry::{
    ModuleDescriptor, ModuleRegistration, Registry, ServiceDescriptor, ServiceEndpoint,
};
pub use service::{ApiService, ApiState};
