//! Stable C ABI for embedding the Mosh interpreter.
//!
//! Returned strings are allocated by Mosh and must be released with
//! `mosh_string_free`. A runtime handle is single-threaded and must not be
//! evaluated and freed concurrently.

use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

use crate::interpreter::Interpreter;
use crate::{lexer, parser};

pub struct MoshRuntime {
    interpreter: Interpreter,
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character < ' ' => {
                escaped.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => escaped.push(character),
        }
    }
    escaped
}

fn json_success(value: &str) -> String {
    format!("{{\"ok\":true,\"value\":\"{}\"}}", json_escape(value))
}

fn json_error(message: &str) -> String {
    format!("{{\"ok\":false,\"error\":\"{}\"}}", json_escape(message))
}

fn into_c_string(value: String) -> *mut c_char {
    CString::new(value)
        .map(CString::into_raw)
        .unwrap_or(ptr::null_mut())
}

/// Create an interpreter runtime using the default backend for this platform.
///
/// # Safety
/// The returned handle must eventually be released with `mosh_runtime_free`.
#[no_mangle]
pub extern "C" fn mosh_runtime_new() -> *mut MoshRuntime {
    catch_unwind(|| {
        Box::into_raw(Box::new(MoshRuntime {
            interpreter: Interpreter::new(),
        }))
    })
    .unwrap_or(ptr::null_mut())
}

/// Destroy a runtime created by `mosh_runtime_new`.
///
/// # Safety
/// `runtime` must be null or a live handle returned by `mosh_runtime_new`.
/// It must not be used again after this call.
#[no_mangle]
pub unsafe extern "C" fn mosh_runtime_free(runtime: *mut MoshRuntime) {
    if runtime.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: upheld by this function's caller contract.
        drop(Box::from_raw(runtime));
    }));
}

/// Evaluate one complete Mosh source string and return a JSON result.
///
/// The returned JSON has either `{"ok":true,"value":"..."}` or
/// `{"ok":false,"error":"..."}`. The returned C string must be released
/// with `mosh_string_free`.
///
/// # Safety
/// `runtime` must be a live handle from `mosh_runtime_new`. `source` must
/// point to a valid NUL-terminated UTF-8 string for the duration of this call.
/// Calls using the same runtime must be serialized by the host.
#[no_mangle]
pub unsafe extern "C" fn mosh_runtime_eval(
    runtime: *mut MoshRuntime,
    source: *const c_char,
) -> *mut c_char {
    if runtime.is_null() {
        return into_c_string(json_error("runtime handle is null"));
    }
    if source.is_null() {
        return into_c_string(json_error("source pointer is null"));
    }

    let evaluated = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: pointer validity and lifetime are part of the caller contract.
        let source = CStr::from_ptr(source)
            .to_str()
            .map_err(|error| format!("source is not valid UTF-8: {error}"))?;
        let tokens = lexer::tokenize(source).map_err(|error| error.to_string())?;
        let program = parser::parse(&tokens).map_err(|error| error.to_string())?;
        // SAFETY: runtime validity is part of the caller contract; calls must be serialized.
        let result = (*runtime)
            .interpreter
            .execute(&program)
            .map_err(|error| error.to_string())?;
        Ok::<String, String>(result.to_string())
    }));

    match evaluated {
        Ok(Ok(value)) => into_c_string(json_success(&value)),
        Ok(Err(error)) => into_c_string(json_error(&error)),
        Err(_) => into_c_string(json_error("internal panic was caught at the Mosh C ABI boundary")),
    }
}

/// Return the Mosh runtime version as an owned C string.
///
/// # Safety
/// The returned string must be released with `mosh_string_free`.
#[no_mangle]
pub extern "C" fn mosh_runtime_version() -> *mut c_char {
    into_c_string(env!("CARGO_PKG_VERSION").to_owned())
}

/// Release a string returned by any Mosh C ABI function.
///
/// # Safety
/// `value` must be null or a pointer previously returned by a Mosh C ABI
/// function and not already released.
#[no_mangle]
pub unsafe extern "C" fn mosh_string_free(value: *mut c_char) {
    if value.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: upheld by this function's caller contract.
        drop(CString::from_raw(value));
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn c_abi_creates_evaluates_and_frees_a_runtime() {
        let runtime = mosh_runtime_new();
        assert!(!runtime.is_null());
        let source = CString::new("let answer = 40 + 2\nanswer\n").unwrap();
        // SAFETY: runtime and source are valid for this call.
        let output = unsafe { mosh_runtime_eval(runtime, source.as_ptr()) };
        assert!(!output.is_null());
        // SAFETY: output is a valid owned string returned by the ABI.
        let json = unsafe { CStr::from_ptr(output) }.to_str().unwrap();
        assert!(json.contains("\"ok\":true"));
        assert!(json.contains("\"value\":\"42\""));
        // SAFETY: output and runtime are each freed exactly once.
        unsafe {
            mosh_string_free(output);
            mosh_runtime_free(runtime);
        }
    }

    #[test]
    fn c_abi_returns_json_errors_for_invalid_source_and_null_handles() {
        let runtime = mosh_runtime_new();
        let source = CString::new("let = broken\n").unwrap();
        // SAFETY: runtime and source are valid.
        let output = unsafe { mosh_runtime_eval(runtime, source.as_ptr()) };
        // SAFETY: output is a valid owned string returned by the ABI.
        let json = unsafe { CStr::from_ptr(output) }.to_str().unwrap();
        assert!(json.contains("\"ok\":false"));
        // SAFETY: output and runtime are each freed exactly once.
        unsafe {
            mosh_string_free(output);
            mosh_runtime_free(runtime);
        }
        // SAFETY: null is explicitly accepted by the ABI.
        let null_result = unsafe { mosh_runtime_eval(ptr::null_mut(), source.as_ptr()) };
        // SAFETY: result is a valid owned string returned by the ABI.
        let json = unsafe { CStr::from_ptr(null_result) }.to_str().unwrap();
        assert!(json.contains("runtime handle is null"));
        // SAFETY: null_result is freed exactly once.
        unsafe { mosh_string_free(null_result) };
    }
}
