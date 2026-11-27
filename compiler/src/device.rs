//! Platform-neutral device runtime contracts.
//!
//! This module deliberately contains no pretend hardware implementations. A host
//! application must register a real platform adapter before a device capability
//! can be reported as supported.

use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Android,
    Ios,
    Windows,
    Linux,
    Unknown,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(target_os = "android") {
            Self::Android
        } else if cfg!(target_os = "ios") {
            Self::Ios
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Unknown
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    NotRequired,
    NotRequested,
    Granted,
    Denied,
    Restricted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    pub module: String,
    pub platform: Platform,
    /// Whether this host has a real adapter for the requested module.
    pub supported: bool,
    /// Whether the resource is currently available (not merely present).
    pub available: bool,
    pub permission: PermissionState,
    pub features: Vec<String>,
    pub limitations: Vec<String>,
}

impl Capability {
    pub fn unsupported(module: impl Into<String>, platform: Platform, reason: impl Into<String>) -> Self {
        Self {
            module: module.into(),
            platform,
            supported: false,
            available: false,
            permission: PermissionState::Unknown,
            features: Vec::new(),
            limitations: vec![reason.into()],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceErrorCode {
    Unsupported,
    NotAvailable,
    PermissionDenied,
    Busy,
    Timeout,
    Disconnected,
    InvalidArgument,
    OsError,
    ResourceClosed,
    LifecycleInterrupted,
    InvalidState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceError {
    pub code: DeviceErrorCode,
    pub module: String,
    pub operation: String,
    pub message: String,
    pub retryable: bool,
}

impl DeviceError {
    pub fn new(
        code: DeviceErrorCode,
        module: impl Into<String>,
        operation: impl Into<String>,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            code,
            module: module.into(),
            operation: operation.into(),
            message: message.into(),
            retryable,
        }
    }

    pub fn unsupported(module: impl Into<String>, operation: impl Into<String>) -> Self {
        let module = module.into();
        Self::new(
            DeviceErrorCode::Unsupported,
            module.clone(),
            operation,
            format!("device module '{module}' is not supported by this host"),
            false,
        )
    }
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?}: {}.{}: {}",
            self.code, self.module, self.operation, self.message
        )
    }
}

impl Error for DeviceError {}

/// Implemented by a native host adapter (Android, iOS, Windows, or Linux).
/// Implementations must query the operating system; they must not infer support
/// from the target platform alone.
pub trait DeviceBackend: Send + Sync {
    fn platform(&self) -> Platform;
    fn capability(&self, module: &str) -> Capability;
    fn permission_state(&self, module: &str) -> Result<PermissionState, DeviceError>;
    fn request_permission(&self, module: &str) -> Result<PermissionState, DeviceError>;
}

/// Safe default for CLI/REPL and hosts that have not installed a native bridge.
#[derive(Debug, Default)]
pub struct UnsupportedBackend;

impl DeviceBackend for UnsupportedBackend {
    fn platform(&self) -> Platform {
        Platform::current()
    }

    fn capability(&self, module: &str) -> Capability {
        Capability::unsupported(
            module,
            self.platform(),
            "no native device adapter is registered",
        )
    }

    fn permission_state(&self, module: &str) -> Result<PermissionState, DeviceError> {
        Err(DeviceError::unsupported(module, "permission_state"))
    }

    fn request_permission(&self, module: &str) -> Result<PermissionState, DeviceError> {
        Err(DeviceError::unsupported(module, "request_permission"))
    }
}

#[derive(Clone)]
pub struct DeviceRegistry {
    backend: Arc<dyn DeviceBackend>,
}

impl fmt::Debug for DeviceRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceRegistry")
            .field("platform", &self.backend.platform())
            .finish_non_exhaustive()
    }
}

impl DeviceRegistry {
    pub fn new(backend: Arc<dyn DeviceBackend>) -> Self {
        Self { backend }
    }

    pub fn unsupported() -> Self {
        Self::new(Arc::new(UnsupportedBackend))
    }

    pub fn platform(&self) -> Platform {
        self.backend.platform()
    }

    pub fn capability(&self, module: &str) -> Capability {
        self.backend.capability(module)
    }

    pub fn permission_state(&self, module: &str) -> Result<PermissionState, DeviceError> {
        let capability = self.backend.capability(module);
        if !capability.supported {
            return Err(DeviceError::unsupported(module, "permission_state"));
        }
        self.backend.permission_state(module)
    }

    pub fn request_permission(&self, module: &str) -> Result<PermissionState, DeviceError> {
        let capability = self.backend.capability(module);
        if !capability.supported {
            return Err(DeviceError::unsupported(module, "request_permission"));
        }
        if !capability.available {
            return Err(DeviceError::new(
                DeviceErrorCode::NotAvailable,
                module,
                "request_permission",
                "the device resource is currently unavailable",
                true,
            ));
        }
        self.backend.request_permission(module)
    }
}

pub trait DeviceResource: Send + Sync {
    fn close(&self) -> Result<(), DeviceError>;
}

/// A host-owned resource handle. Closing is idempotent; Drop is a best-effort
/// cleanup fallback, while callers should still close explicitly to observe errors.
pub struct DeviceHandle {
    id: String,
    module: String,
    resource: Arc<dyn DeviceResource>,
    closed: AtomicBool,
}

impl fmt::Debug for DeviceHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceHandle")
            .field("id", &self.id)
            .field("module", &self.module)
            .field("closed", &self.is_closed())
            .finish()
    }
}

impl DeviceHandle {
    pub fn new(id: impl Into<String>, module: impl Into<String>, resource: Arc<dyn DeviceResource>) -> Self {
        Self {
            id: id.into(),
            module: module.into(),
            resource,
            closed: AtomicBool::new(false),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn module(&self) -> &str {
        &self.module
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    pub fn ensure_open(&self, operation: &str) -> Result<(), DeviceError> {
        if self.is_closed() {
            Err(DeviceError::new(
                DeviceErrorCode::ResourceClosed,
                self.module.clone(),
                operation,
                format!("device handle '{}' is closed", self.id),
                false,
            ))
        } else {
            Ok(())
        }
    }

    pub fn close(&self) -> Result<(), DeviceError> {
        if self
            .closed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(());
        }
        if let Err(error) = self.resource.close() {
            self.closed.store(false, Ordering::Release);
            return Err(error);
        }
        Ok(())
    }
}

impl Drop for DeviceHandle {
    fn drop(&mut self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            let _ = self.resource.close();
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    Created,
    PermissionPending,
    Ready,
    Active,
    Interrupted,
    Closed,
}

#[derive(Debug, Clone)]
pub struct Lifecycle {
    state: Arc<Mutex<LifecycleState>>,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl Lifecycle {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(LifecycleState::Created)),
        }
    }

    pub fn state(&self) -> LifecycleState {
        *self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn transition(&self, next: LifecycleState, module: &str) -> Result<(), DeviceError> {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let valid = matches!(
            (*state, next),
            (LifecycleState::Created, LifecycleState::PermissionPending)
                | (LifecycleState::Created, LifecycleState::Ready)
                | (LifecycleState::Created, LifecycleState::Closed)
                | (LifecycleState::PermissionPending, LifecycleState::Ready)
                | (LifecycleState::PermissionPending, LifecycleState::Interrupted)
                | (LifecycleState::PermissionPending, LifecycleState::Closed)
                | (LifecycleState::Ready, LifecycleState::Active)
                | (LifecycleState::Ready, LifecycleState::Interrupted)
                | (LifecycleState::Ready, LifecycleState::Closed)
                | (LifecycleState::Active, LifecycleState::Ready)
                | (LifecycleState::Active, LifecycleState::Interrupted)
                | (LifecycleState::Active, LifecycleState::Closed)
                | (LifecycleState::Interrupted, LifecycleState::Ready)
                | (LifecycleState::Interrupted, LifecycleState::Closed)
        );
        if !valid {
            return Err(DeviceError::new(
                DeviceErrorCode::InvalidState,
                module,
                "lifecycle_transition",
                format!("invalid device lifecycle transition: {:?} -> {:?}", *state, next),
                false,
            ));
        }
        *state = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingResource(AtomicUsize);

    impl DeviceResource for CountingResource {
        fn close(&self) -> Result<(), DeviceError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn unregistered_host_reports_unsupported_instead_of_fake_support() {
        let registry = DeviceRegistry::unsupported();
        let capability = registry.capability("device.camera");
        assert!(!capability.supported);
        assert!(!capability.available);
        assert!(capability.limitations[0].contains("no native device adapter"));
        assert_eq!(
            registry.request_permission("device.camera").unwrap_err().code,
            DeviceErrorCode::Unsupported
        );
    }

    #[test]
    fn cancellation_token_is_shared_and_one_way() {
        let first = CancellationToken::new();
        let second = first.clone();
        assert!(!second.is_cancelled());
        first.cancel();
        assert!(second.is_cancelled());
    }

    #[test]
    fn handle_close_is_idempotent_and_closed_handles_reject_use() {
        let resource = Arc::new(CountingResource(AtomicUsize::new(0)));
        let handle = DeviceHandle::new("camera-1", "device.camera", resource.clone());
        handle.ensure_open("capture").unwrap();
        handle.close().unwrap();
        handle.close().unwrap();
        assert!(handle.is_closed());
        assert_eq!(resource.0.load(Ordering::SeqCst), 1);
        assert_eq!(
            handle.ensure_open("capture").unwrap_err().code,
            DeviceErrorCode::ResourceClosed
        );
    }

    #[test]
    fn lifecycle_enforces_valid_transitions() {
        let lifecycle = Lifecycle::new();
        lifecycle.transition(LifecycleState::Ready, "device.camera").unwrap();
        lifecycle.transition(LifecycleState::Active, "device.camera").unwrap();
        lifecycle.transition(LifecycleState::Interrupted, "device.camera").unwrap();
        lifecycle.transition(LifecycleState::Ready, "device.camera").unwrap();
        lifecycle.transition(LifecycleState::Closed, "device.camera").unwrap();
        assert_eq!(lifecycle.state(), LifecycleState::Closed);
        assert_eq!(
            lifecycle.transition(LifecycleState::Active, "device.camera").unwrap_err().code,
            DeviceErrorCode::InvalidState
        );
    }
}
