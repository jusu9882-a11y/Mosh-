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

    fn read_storage_volumes(&self) -> Result<Vec<StorageVolume>, DeviceError> {
        if !matches!(self.platform, Platform::Android | Platform::Linux) {
            return Err(DeviceError::unsupported("device.storage", "volumes"));
        }
        self.read_storage_from_proc()
    }

    fn read_memory(&self) -> Result<MemoryReading, DeviceError> {
        Err(DeviceError::unsupported("device.memory", "read"))
    }

    fn read_cpu(&self) -> Result<CpuReading, DeviceError> {
        Err(DeviceError::unsupported("device.cpu", "read"))
    }

    fn read_storage_volumes(&self) -> Result<Vec<StorageVolume>, DeviceError> {
        Err(DeviceError::unsupported("device.storage", "volumes"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageVolume {
    pub source: String,
    pub mount_point: String,
    pub filesystem: String,
    pub options: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryReading {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub free_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_free_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CpuReading {
    pub model: String,
    pub architecture: String,
    pub logical_cores: usize,
    pub frequency_mhz: Option<f64>,
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
    proc_root: PathBuf,
    platform: Platform,
}

impl SysfsBackend {
    pub fn system() -> Self {
        Self::with_root("/sys/class/power_supply", Platform::current())
    }

    pub fn with_root(root: impl Into<PathBuf>, platform: Platform) -> Self {
        Self::with_paths(root, "/proc", platform)
    }

    pub fn with_paths(root: impl Into<PathBuf>, proc_root: impl Into<PathBuf>, platform: Platform) -> Self {
        Self { root: root.into(), proc_root: proc_root.into(), platform }
    }

    fn battery_path(&self) -> Option<PathBuf> {
        if !matches!(self.platform, Platform::Android | Platform::Linux) {
            return None;
        }
        let entries = fs::read_dir(&self.root).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = fs::read_to_string(path.join("type")) else {
                continue;
            };
            if kind.trim().eq_ignore_ascii_case("battery") {
                return Some(path);
            }
        }
        None
    }

    fn read_storage_from_proc(&self) -> Result<Vec<StorageVolume>, DeviceError> {
        let contents = fs::read_to_string(self.proc_root.join("mounts")).map_err(|error| {
            DeviceError::new(DeviceErrorCode::NotAvailable, "device.storage", "volumes", format!("cannot read /proc/mounts: {error}"), true)
        })?;
        let mut volumes = Vec::new();
        for line in contents.lines() {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 4 {
                continue;
            }
            let options = fields[3].to_owned();
            volumes.push(StorageVolume {
                source: decode_mount_field(fields[0]),
                mount_point: decode_mount_field(fields[1]),
                filesystem: fields[2].to_owned(),
                read_only: options.split(',').any(|option| option == "ro"),
                options,
            });
        }
        Ok(volumes)
    }

    fn read_memory_from_proc(&self) -> Result<MemoryReading, DeviceError> {
        let contents = fs::read_to_string(self.proc_root.join("meminfo")).map_err(|error| {
            DeviceError::new(DeviceErrorCode::NotAvailable, "device.memory", "read", format!("cannot read /proc/meminfo: {error}"), true)
        })?;
        let mut fields = std::collections::HashMap::new();
        for line in contents.lines() {
            if let Some((key, value)) = line.split_once(':') {
                if let Some(number) = value.split_whitespace().next().and_then(|v| v.parse::<u64>().ok()) {
                    fields.insert(key.trim(), number.saturating_mul(1024));
                }
            }
        }
        let get = |key: &str| fields.get(key).copied();
        let total_bytes = get("MemTotal").ok_or_else(|| DeviceError::new(DeviceErrorCode::NotAvailable, "device.memory", "read", "MemTotal is missing from /proc/meminfo", true))?;
        let free_bytes = get("MemFree").ok_or_else(|| DeviceError::new(DeviceErrorCode::NotAvailable, "device.memory", "read", "MemFree is missing from /proc/meminfo", true))?;
        let available_bytes = get("MemAvailable").unwrap_or(free_bytes);
        Ok(MemoryReading {
            total_bytes,
            available_bytes,
            free_bytes,
            swap_total_bytes: get("SwapTotal").unwrap_or(0),
            swap_free_bytes: get("SwapFree").unwrap_or(0),
        })
    }

    fn read_cpu_from_proc(&self) -> Result<CpuReading, DeviceError> {
        let contents = fs::read_to_string(self.proc_root.join("cpuinfo")).map_err(|error| {
            DeviceError::new(DeviceErrorCode::NotAvailable, "device.cpu", "read", format!("cannot read /proc/cpuinfo: {error}"), true)
        })?;
        let mut model = None;
        let mut frequency_mhz = None;
        let mut logical_cores = 0usize;
        for line in contents.lines() {
            let Some((key, value)) = line.split_once(':') else { continue; };
            let key = key.trim();
            let value = value.trim();
            if matches!(key, "model name" | "Hardware" | "Processor" | "cpu model") && model.is_none() {
                model = Some(value.to_owned());
            }
            if key == "processor" {
                logical_cores += 1;
            }
            if key == "cpu MHz" && frequency_mhz.is_none() {
                frequency_mhz = value.parse::<f64>().ok();
            }
        }
        let model = model.unwrap_or_else(|| format!("{} processor", std::env::consts::ARCH));
        if logical_cores == 0 {
            logical_cores = std::thread::available_parallelism().map(usize::from).unwrap_or(1);
        }
        Ok(CpuReading {
            model,
            architecture: std::env::consts::ARCH.to_owned(),
            logical_cores,
            frequency_mhz,
        })
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
        if !matches!(self.platform, Platform::Android | Platform::Linux) {
            return Capability::unsupported(module, self.platform, "this procfs/sysfs adapter is available only on Linux and Android");
        }
        if module == "device.storage" || module == "device.fs" {
            return match self.read_storage_from_proc() {
                Ok(volumes) => Capability {
                    module: module.into(), platform: self.platform, supported: true, available: !volumes.is_empty(),
                    permission: PermissionState::NotRequired,
                    features: vec!["volumes".into(), "mount_points".into(), "filesystem_types".into(), "read_only_flags".into()],
                    limitations: Vec::new(),
                },
                Err(error) => Capability::unsupported(module, self.platform, error.message),
            };
        }
        if module == "device.memory" || module == "device.ram" {
            return match self.read_memory_from_proc() {
                Ok(_) => Capability {
                    module: module.into(), platform: self.platform, supported: true, available: true,
                    permission: PermissionState::NotRequired,
                    features: vec!["total_bytes".into(), "available_bytes".into(), "free_bytes".into(), "swap_total_bytes".into(), "swap_free_bytes".into()],
                    limitations: Vec::new(),
                },
                Err(error) => Capability::unsupported(module, self.platform, error.message),
            };
        }
        if module == "device.cpu" {
            return match self.read_cpu_from_proc() {
                Ok(reading) => Capability {
                    module: module.into(), platform: self.platform, supported: true, available: reading.logical_cores > 0,
                    permission: PermissionState::NotRequired,
                    features: vec!["model".into(), "architecture".into(), "logical_cores".into(), "frequency_mhz".into()],
                    limitations: Vec::new(),
                },
                Err(error) => Capability::unsupported(module, self.platform, error.message),
            };
        }
        if module != "device.battery" {
            return Capability::unsupported(module, self.platform, "this sysfs/procfs adapter implements battery, memory, and CPU telemetry only");
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

    fn read_memory(&self) -> Result<MemoryReading, DeviceError> {
        if !matches!(self.platform, Platform::Android | Platform::Linux) {
            return Err(DeviceError::unsupported("device.memory", "read"));
        }
        self.read_memory_from_proc()
    }

    fn read_cpu(&self) -> Result<CpuReading, DeviceError> {
        if !matches!(self.platform, Platform::Android | Platform::Linux) {
            return Err(DeviceError::unsupported("device.cpu", "read"));
        }
        self.read_cpu_from_proc()
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

fn decode_mount_field(value: &str) -> String {
    value.replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
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

    pub fn read_storage_volumes(&self) -> Result<Vec<StorageVolume>, DeviceError> {
        let capability = self.backend.capability("device.storage");
        if !capability.supported {
            return Err(DeviceError::unsupported("device.storage", "volumes"));
        }
        if !capability.available {
            return Err(DeviceError::new(DeviceErrorCode::NotAvailable, "device.storage", "volumes", "no mounted storage volumes were reported", true));
        }
        self.backend.read_storage_volumes()
    }

    pub fn read_memory(&self) -> Result<MemoryReading, DeviceError> {
        let capability = self.backend.capability("device.memory");
        if !capability.supported || !capability.available {
            return Err(DeviceError::new(DeviceErrorCode::NotAvailable, "device.memory", "read", "memory telemetry is unavailable on this host", false));
        }
        self.backend.read_memory()
    }

    pub fn read_cpu(&self) -> Result<CpuReading, DeviceError> {
        let capability = self.backend.capability("device.cpu");
        if !capability.supported || !capability.available {
            return Err(DeviceError::new(DeviceErrorCode::NotAvailable, "device.cpu", "read", "CPU telemetry is unavailable on this host", false));
        }
        self.backend.read_cpu()
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
    fn procfs_backend_reads_memory_and_cpu_telemetry() {
        let root = std::env::temp_dir().join(format!("mosh-proc-fixture-{}", std::process::id()));
        let power = root.join("power");
        let proc = root.join("proc");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&power).unwrap();
        fs::create_dir_all(&proc).unwrap();
        fs::write(proc.join("meminfo"), "MemTotal:       8192000 kB\nMemFree:        1024000 kB\nMemAvailable:   2048000 kB\nSwapTotal:       512000 kB\nSwapFree:        256000 kB\n").unwrap();
        fs::write(proc.join("cpuinfo"), "processor : 0\nmodel name : Test ARM CPU\ncpu MHz : 2400.000\n\nprocessor : 1\nmodel name : Test ARM CPU\ncpu MHz : 2400.000\n").unwrap();
        let backend = SysfsBackend::with_paths(&power, &proc, Platform::Linux);
        let memory = backend.read_memory().unwrap();
        assert_eq!(memory.total_bytes, 8_388_608_000);
        assert_eq!(memory.available_bytes, 2_097_152_000);
        assert_eq!(memory.free_bytes, 1_048_576_000);
        assert_eq!(memory.swap_total_bytes, 524_288_000);
        assert_eq!(memory.swap_free_bytes, 262_144_000);
        let cpu = backend.read_cpu().unwrap();
        assert_eq!(cpu.model, "Test ARM CPU");
        assert_eq!(cpu.logical_cores, 2);
        assert_eq!(cpu.frequency_mhz, Some(2400.0));
        assert!(backend.capability("device.memory").supported);
        assert!(backend.capability("device.cpu").supported);
        assert!(backend.capability("device.ram").supported);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sysfs_backend_reads_realistic_battery_attributes_and_units() {
        let root = std::env::temp_dir().join(format!("mosh-sysfs-battery-{}", std::process::id()));
        let battery = root.join("BAT0");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&battery).unwrap();
        fs::write(battery.join("type"), "Battery\n").unwrap();
        fs::write(battery.join("present"), "1\n").unwrap();
        fs::write(battery.join("capacity"), "87\n").unwrap();
        fs::write(battery.join("status"), "Discharging\n").unwrap();
        fs::write(battery.join("voltage_now"), "4123000\n").unwrap();
        fs::write(battery.join("current_now"), "-850000\n").unwrap();
        fs::write(battery.join("temp"), "315\n").unwrap();
        fs::write(battery.join("technology"), "Li-ion\n").unwrap();

        let backend = SysfsBackend::with_root(&root, Platform::Linux);
        let capability = backend.capability("device.battery");
        assert!(capability.supported);
        assert!(capability.available);
        assert_eq!(capability.permission, PermissionState::NotRequired);
        let reading = backend.read_battery().unwrap();
        assert_eq!(reading.capacity_percent, Some(87.0));
        assert_eq!(reading.state.as_deref(), Some("Discharging"));
        assert_eq!(reading.voltage_mv, Some(4123.0));
        assert_eq!(reading.current_ma, Some(-850.0));
        assert_eq!(reading.temperature_c, Some(31.5));
        assert_eq!(reading.technology.as_deref(), Some("Li-ion"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sysfs_backend_does_not_claim_battery_support_when_no_battery_is_present() {
        let root = std::env::temp_dir().join(format!("mosh-sysfs-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let backend = SysfsBackend::with_root(&root, Platform::Linux);
        let capability = backend.capability("device.battery");
        assert!(!capability.supported);
        assert!(!capability.available);
        assert_eq!(backend.read_battery().unwrap_err().code, DeviceErrorCode::NotAvailable);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sysfs_backend_rejects_platforms_without_sysfs_adapter_support() {
        let backend = SysfsBackend::with_root("/sys/class/power_supply", Platform::Windows);
        assert!(!backend.capability("device.battery").supported);
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
