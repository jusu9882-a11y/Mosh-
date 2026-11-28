//! Runtime catalog for built-in Mosh module namespaces.
//!
//! The catalog distinguishes namespaces that are currently usable from those
//! reserved for future runtime domains. It is metadata, not a claim that every
//! listed submodule or platform adapter has been implemented.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDescriptor {
    pub namespace: &'static str,
    pub implemented: bool,
    pub description: &'static str,
    pub modules: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub struct ModuleRegistry {
    descriptors: Vec<ModuleDescriptor>,
}

impl Default for ModuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self {
            descriptors: vec![
                ModuleDescriptor {
                    namespace: "std",
                    implemented: true,
                    description: "Core language and standard-library facilities",
                    modules: &["strings", "collections", "math", "files", "paths", "json", "time", "env"],
                },
                ModuleDescriptor {
                    namespace: "device",
                    implemented: true,
                    description: "Capability-checked hardware and device services",
                    modules: &["battery", "memory", "ram", "cpu", "storage", "fs"],
                },
                ModuleDescriptor {
                    namespace: "web",
                    implemented: false,
                    description: "HTTP, WebSocket, and web-server integrations",
                    modules: &["http", "websocket", "server"],
                },
                ModuleDescriptor {
                    namespace: "app",
                    implemented: false,
                    description: "Application lifecycle, navigation, and UI host integrations",
                    modules: &["lifecycle", "navigation", "notifications"],
                },
                ModuleDescriptor {
                    namespace: "ai",
                    implemented: false,
                    description: "Model inference, embeddings, and AI tool integrations",
                    modules: &["models", "embeddings", "tools"],
                },
                ModuleDescriptor {
                    namespace: "data",
                    implemented: false,
                    description: "Databases and structured data processing",
                    modules: &["sqlite", "csv", "stream"],
                },
                ModuleDescriptor {
                    namespace: "science",
                    implemented: false,
                    description: "Numerical, scientific, and engineering computation",
                    modules: &["stats", "signal", "simulation"],
                },
                ModuleDescriptor {
                    namespace: "security",
                    implemented: false,
                    description: "Cryptography and defensive security utilities",
                    modules: &["crypto", "audit", "tls"],
                },
                ModuleDescriptor {
                    namespace: "net",
                    implemented: false,
                    description: "Low-level network protocols and services",
                    modules: &["tcp", "udp", "dns"],
                },
            ],
        }
    }

    pub fn namespaces(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.descriptors.iter().map(|item| item.namespace)
    }

    pub fn get(&self, namespace: &str) -> Option<&ModuleDescriptor> {
        self.descriptors.iter().find(|item| item.namespace == namespace)
    }

    pub fn implemented_namespaces(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.descriptors.iter().filter(|item| item.implemented).map(|item| item.namespace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_unique_namespaces() {
        let registry = ModuleRegistry::new();
        let names: Vec<_> = registry.namespaces().collect();
        let unique: std::collections::HashSet<_> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len());
    }

    #[test]
    fn implemented_and_reserved_namespaces_are_distinguished() {
        let registry = ModuleRegistry::new();
        assert_eq!(registry.get("device").unwrap().implemented, true);
        assert_eq!(registry.get("std").unwrap().implemented, true);
        assert_eq!(registry.get("web").unwrap().implemented, false);
        assert!(registry.get("not_a_namespace").is_none());
    }
}
