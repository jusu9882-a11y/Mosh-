//! Platform-neutral device runtime contracts.
//!
//! This module deliberately contains no pretend hardware implementations. A host
//! application must register a real platform adapter before a device capability
//! can be reported as supported.

use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::fs;
use std::path::{Path, PathBuf};

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

    fn read_battery(&self) -> Result<BatteryReading, DeviceError> {
        Err(DeviceError::unsupported("device.battery", "read"))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatteryReading {
    pub present: bool,
    pub capacity_percent: Option<f64>,
    pub state: Option<String>,
    pub voltage_mv: Option<f64>,
    pub current_ma: Option<f64>,
    pub temperature_c: Option<f64>,
    pub technology: Option<String>,
}

/// Linux/Android sysfs battery adapter. It reads kernel-exposed power-supply
/// attributes only; it never fabricates readings or prompts for permissions.
#[derive(Debug, Clone)]
pub struct SysfsBackend {
    root: PathBuf,
    platform: Platform,
}

impl SysfsBackend {
    pub fn system() -> Self {
        Self::with_root("/sys/class/power_supply", Platform::current())
    }

    pub fn with_root(root: impl Into<PathBuf>, platform: Platform) -> Self {
        Self { root: root.into(), platform }
    }

    fn battery_path(&self) -> Option<PathBuf> {
        if !matches!(self.platform, Platform::Android | Platform::Linux) {
            return None;
        }
        let entries = fs::read_dir(&self.root).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            let kind = fs::read_to_string(path.join("type")).ok()?;
            if kind.trim().eq_ignore_ascii_case("battery") {
                return Some(path);
            }
        }
        None
    }

    fn read_optional_number(path: &Path, name: &str) -> Option<f64> {
        fs::read_to_string(path.join(name)).ok()?.trim().parse::<f64>().ok()
    }

    fn read_optional_text(path: &Path, name: &str) -> Option<String> {
        let value = fs::read_to_string(path.join(name)).ok()?;
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    }
}

impl DeviceBackend for SysfsBackend {
    fn platform(&self) -> Platform {
        self.platform
    }

    fn capability(&self, module: &str) -> Capability {
        if module != "device.battery" {
            return Capability::unsupported(module, self.platform, "this sysfs adapter implements battery telemetry only");
        }
        let Some(path) = self.battery_path() else {
            return Capability::unsupported(module, self.platform, "no readable battery device was detected through power_supply sysfs");
        };
        let present = Self::read_optional_text(&path, "present")
            .map(|value| value == "1")
            .unwrap_or(true);
        let readable = path.join("capacity").is_file();
        Capability {
            module: module.into(),
            platform: self.platform,
            supported: true,
            available: present && readable,
            permission: PermissionState::NotRequired,
            features: ["capacity", "charging_state", "voltage", "current", "temperature", "technology"]
                .into_iter().filter(|feature| match *feature {
                    "capacity" => path.join("capacity").is_file(),
                    "charging_state" => path.join("status").is_file(),
                    "voltage" => path.join("voltage_now").is_file(),
                    "current" => path.join("current_now").is_file(),
                    "temperature" => path.join("temp").is_file(),
                    "technology" => path.join("technology").is_file(),
                    _ => false,
                }).map(str::to_owned).collect(),
            limitations: if readable { Vec::new() } else { vec!["battery capacity is not readable from sysfs".into()] },
        }
    }

    fn permission_state(&self, module: &str) -> Result<PermissionState, DeviceError> {
        if !self.capability(module).supported {
            return Err(DeviceError::unsupported(module, "permission_state"));
        }
        Ok(PermissionState::NotRequired)
    }

    fn request_permission(&self, module: &str) -> Result<PermissionState, DeviceError> {
        if !self.capability(module).supported {
            return Err(DeviceError::unsupported(module, "request_permission"));
        }
        Ok(PermissionState::NotRequired)
    }

    fn read_battery(&self) -> Result<BatteryReading, DeviceError> {
        let path = self.battery_path().ok_or_else(|| DeviceError::new(
            DeviceErrorCode::NotAvailable, "device.battery", "read",
            "no battery device is exposed by the host", false
        ))?;
        let present = Self::read_optional_text(&path, "present")
            .map(|value| value == "1").unwrap_or(true);
        if !present {
            return Err(DeviceError::new(DeviceErrorCode::NotAvailable, "device.battery", "read", "battery is reported as absent", false));
        }
        let capacity = Self::read_optional_number(&path, "capacity");
        let voltage = Self::read_optional_number(&path, "voltage_now").map(|value| value / 1000.0);
        let current = Self::read_optional_number(&path, "current_now").map(|value| value / 1000.0);
        let temperature = Self::read_optional_number(&path, "temp").map(|value| value / 10.0);
        let state = Self::read_optional_text(&path, "status");
        let technology = Self::read_optional_text(&path, "technology");
        if capacity.is_none() && state.is_none() && voltage.is_none() && current.is_none() && temperature.is_none() {
            return Err(DeviceError::new(DeviceErrorCode::NotAvailable, "device.battery", "read", "battery telemetry attributes are not readable", true));
        }
        Ok(BatteryReading {
            present,
            capacity_percent: capacity,
            state,
            voltage_mv: voltage,
            current_ma: current,
            temperature_c: temperature,
            technology,
        })
    }
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

    pub fn system_default() -> Self {
        match Platform::current() {
            Platform::Android | Platform::Linux => Self::new(Arc::new(SysfsBackend::system())),
            _ => Self::unsupported(),
        }
    }

    pub fn read_battery(&self) -> Result<BatteryReading, DeviceError> {
        let capability = self.backend.capability("device.battery");
        if !capability.supported {
            return Err(DeviceError::unsupported("device.battery", "read"));
        }
        if !capability.available {
            return Err(DeviceError::new(
                DeviceErrorCode::NotAvailable,
                "device.battery",
                "read",
                "battery telemetry is currently unavailable",
                true,
            ));
        }
        self.backend.read_battery()
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
