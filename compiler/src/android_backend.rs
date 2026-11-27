//! Android framework-backed device adapter.
//!
//! The Kotlin host owns Android framework access and exposes narrow callbacks.
//! Rust uses JNI global references and attaches the calling thread for each call.

use std::collections::HashMap;

use jni::objects::{GlobalRef, JObject, JString};
use jni::JavaVM;

use crate::device::{
    Capability, DeviceBackend, DeviceError, DeviceErrorCode, BatteryReading, PermissionState, Platform,
};

pub struct AndroidBackend {
    vm: JavaVM,
    host: GlobalRef,
}

impl AndroidBackend {
    pub fn new(env: &mut jni::JNIEnv<'_>, host: &JObject<'_>) -> Result<Self, DeviceError> {
        let vm = env.get_java_vm().map_err(|error| jni_error("get_java_vm", error))?;
        let host = env.new_global_ref(host).map_err(|error| jni_error("new_global_ref", error))?;
        Ok(Self { vm, host })
    }

    fn call_bool(&self, method: &str) -> Result<bool, DeviceError> {
        let mut env = self.vm.attach_current_thread()
            .map_err(|error| jni_error(method, error))?;
        let value = env.call_method(self.host.as_obj(), method, "()Z", &[])
            .map_err(|error| jni_error(method, error))?;
        value.z().map_err(|error| jni_error(method, error))
    }

    fn call_string(&self, method: &str) -> Result<String, DeviceError> {
        let mut env = self.vm.attach_current_thread()
            .map_err(|error| jni_error(method, error))?;
        let value = env.call_method(
            self.host.as_obj(),
            method,
            "()Ljava/lang/String;",
            &[],
        ).map_err(|error| jni_error(method, error))?.l()
            .map_err(|error| jni_error(method, error))?;
        if value.is_null() {
            return Err(DeviceError::new(
                DeviceErrorCode::NotAvailable,
                "device.battery",
                method,
                "Android host returned a null battery response",
                true,
            ));
        }
        let value = JString::from(value);
        let value: String = env.get_string(&value)
            .map_err(|error| jni_error(method, error))?
            .into();
        Ok(value)
    }
}

fn jni_error(operation: &str, error: impl std::fmt::Display) -> DeviceError {
    DeviceError::new(
        DeviceErrorCode::OsError,
        "device.battery",
        operation,
        format!("Android JNI host callback failed: {error}"),
        true,
    )
}

fn parse_battery_data(data: &str) -> Result<BatteryReading, DeviceError> {
    let fields = data.split(';').filter_map(|field| {
        let (key, value) = field.split_once('=')?;
        Some((key.trim().to_owned(), value.trim().to_owned()))
    }).collect::<HashMap<_, _>>();
    let get = |key: &str| fields.get(key).map(String::as_str);
    let parse_number = |key: &str| get(key).filter(|value| !value.is_empty()).and_then(|value| value.parse::<f64>().ok());
    let present = get("present").map(|value| value == "true").unwrap_or(false);
    let reading = BatteryReading {
        present,
        capacity_percent: parse_number("capacity_percent"),
        state: get("state").filter(|value| !value.is_empty()).map(str::to_owned),
        voltage_mv: parse_number("voltage_mv"),
        current_ma: parse_number("current_ma"),
        temperature_c: parse_number("temperature_c"),
        technology: get("technology").filter(|value| !value.is_empty()).map(str::to_owned),
    };
    if !reading.present {
        return Err(DeviceError::new(
            DeviceErrorCode::NotAvailable,
            "device.battery",
            "read",
            "Android reports that the battery is not present",
            false,
        ));
    }
    if reading.capacity_percent.is_none()
        && reading.state.is_none()
        && reading.voltage_mv.is_none()
        && reading.current_ma.is_none()
        && reading.temperature_c.is_none()
    {
        return Err(DeviceError::new(
            DeviceErrorCode::NotAvailable,
            "device.battery",
            "read",
            "Android did not provide readable battery telemetry",
            true,
        ));
    }
    Ok(reading)
}

impl DeviceBackend for AndroidBackend {
    fn platform(&self) -> Platform {
        Platform::Android
    }

    fn capability(&self, module: &str) -> Capability {
        if module != "device.battery" {
            return Capability::unsupported(
                module,
                Platform::Android,
                "Android framework adapter currently implements battery telemetry only",
            );
        }
        let supported = self.call_bool("batterySupported").unwrap_or(false);
        if !supported {
            return Capability::unsupported(
                module,
                Platform::Android,
                "Android BatteryManager is not available from the host",
            );
        }
        let available = self.call_bool("batteryAvailable").unwrap_or(false);
        Capability {
            module: module.into(),
            platform: Platform::Android,
            supported: true,
            available,
            permission: PermissionState::NotRequired,
            features: vec![
                "capacity".into(),
                "charging_state".into(),
                "voltage".into(),
                "current".into(),
                "temperature".into(),
                "technology".into(),
            ],
            limitations: Vec::new(),
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
        if !self.call_bool("batteryAvailable")? {
            return Err(DeviceError::new(
                DeviceErrorCode::NotAvailable,
                "device.battery",
                "read",
                "Android battery telemetry is currently unavailable",
                true,
            ));
        }
        parse_battery_data(&self.call_string("batteryData")?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_android_battery_fields_and_optional_values() {
        let reading = parse_battery_data(
            "present=true;capacity_percent=76;state=Charging;voltage_mv=4012;current_ma=525.5;temperature_c=29.8;technology=Li-ion"
        ).unwrap();
        assert_eq!(reading.capacity_percent, Some(76.0));
        assert_eq!(reading.state.as_deref(), Some("Charging"));
        assert_eq!(reading.voltage_mv, Some(4012.0));
        assert_eq!(reading.current_ma, Some(525.5));
        assert_eq!(reading.temperature_c, Some(29.8));
        assert_eq!(reading.technology.as_deref(), Some("Li-ion"));
    }

    #[test]
    fn rejects_android_battery_absence_and_empty_telemetry() {
        assert_eq!(
            parse_battery_data("present=false").unwrap_err().code,
            DeviceErrorCode::NotAvailable
        );
        assert_eq!(
            parse_battery_data("present=true;capacity_percent=").unwrap_err().code,
            DeviceErrorCode::NotAvailable
        );
    }
}
