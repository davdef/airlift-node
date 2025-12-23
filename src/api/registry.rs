use serde::Serialize;
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Clone, Serialize)]
pub struct ServiceEndpoint {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Serialize)]
pub struct ModuleDescriptor {
    pub id: String,
    #[serde(rename = "type")]
    pub module_type: String,
    pub desired_state: String,
    pub runtime_state: String,
    pub metrics: Option<serde_json::Value>,
    pub last_error: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct ModuleRegistration {
    pub descriptor: ModuleDescriptor,
    pub controls: Vec<String>,
    pub endpoints: Vec<ServiceEndpoint>,
}

#[derive(Clone, Serialize)]
pub struct ServiceDescriptor {
    pub id: String,
    #[serde(rename = "type")]
    pub service_type: String,
    pub core: bool,
    pub endpoints: Vec<ServiceEndpoint>,
}

#[derive(Default)]
pub struct Registry {
    modules: RwLock<HashMap<String, ModuleRegistration>>,
    services: RwLock<HashMap<String, ServiceDescriptor>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            modules: RwLock::new(HashMap::new()),
            services: RwLock::new(HashMap::new()),
        }
    }

    pub fn register_module(&self, module: ModuleRegistration) {
        self.modules
            .write()
            .expect("modules registry lock")
            .insert(module.descriptor.id.clone(), module);
    }

    pub fn register_service(&self, service: ServiceDescriptor) {
        self.services
            .write()
            .expect("services registry lock")
            .insert(service.id.clone(), service);
    }

    pub fn list_modules(&self) -> Vec<ModuleRegistration> {
        self.modules
            .read()
            .expect("modules registry lock")
            .values()
            .cloned()
            .collect()
    }

    pub fn list_services(&self) -> Vec<ServiceDescriptor> {
        self.services
            .read()
            .expect("services registry lock")
            .values()
            .cloned()
            .collect()
    }
}

impl ModuleDescriptor {
    pub fn new(
        id: impl Into<String>,
        module_type: impl Into<String>,
        desired_state: impl Into<String>,
        runtime_state: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            module_type: module_type.into(),
            desired_state: desired_state.into(),
            runtime_state: runtime_state.into(),
            metrics: None,
            last_error: None,
        }
    }
}

impl ModuleRegistration {
    pub fn new(descriptor: ModuleDescriptor) -> Self {
        Self {
            descriptor,
            controls: Vec::new(),
            endpoints: Vec::new(),
        }
    }
}
