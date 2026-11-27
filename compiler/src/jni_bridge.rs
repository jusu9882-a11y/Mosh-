//! JNI entry points for the Android Kotlin host wrapper.
//!
//! The Kotlin class name is com.jusu.mosh.MoshRuntime. The JNI layer delegates
//! to the same panic-safe C ABI used by other native hosts.

use std::ffi::{CStr, CString};
use std::ptr;

use jni::objects::{JObject, JString};
use jni::sys::{jlong, jstring};
use jni::JNIEnv;

use crate::ffi::{
    mosh_runtime_eval, mosh_runtime_free, mosh_runtime_new, mosh_runtime_version,
    mosh_string_free, MoshRuntime,
};

fn java_string(env: &mut JNIEnv, value: impl Into<String>) -> jstring {
    env.new_string(value.into())
        .map(|value| value.into_raw())
        .unwrap_or(ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_jusu_mosh_MoshRuntime_nativeCreate(
    _env: JNIEnv,
    _this: JObject,
) -> jlong {
    mosh_runtime_new() as jlong
}

#[no_mangle]
pub extern "system" fn Java_com_jusu_mosh_MoshRuntime_nativeFree(
    _env: JNIEnv,
    _this: JObject,
    handle: jlong,
) {
    if handle != 0 {
        let runtime = handle as usize as *mut MoshRuntime;
        // SAFETY: Kotlin owns this handle and calls nativeFree exactly once.
        unsafe { mosh_runtime_free(runtime) };
    }
}

#[no_mangle]
pub extern "system" fn Java_com_jusu_mosh_MoshRuntime_nativeEval(
    mut env: JNIEnv,
    _this: JObject,
    handle: jlong,
    source: JString,
) -> jstring {
    if handle == 0 {
        return java_string(&mut env, r#"{"ok":false,"error":"runtime handle is closed"}"#);
    }
    let source: String = match env.get_string(&source) {
        Ok(value) => value.into(),
        Err(_) => return java_string(&mut env, r#"{"ok":false,"error":"could not read source string"}"#),
    };
    let source = match CString::new(source) {
        Ok(source) => source,
        Err(_) => return java_string(&mut env, r#"{"ok":false,"error":"source contains a NUL character"}"#),
    };
    let runtime = handle as usize as *mut MoshRuntime;
    // SAFETY: the Kotlin wrapper serializes access and retains the runtime until close().
    let result = unsafe { mosh_runtime_eval(runtime, source.as_ptr()) };
    if result.is_null() {
        return java_string(&mut env, r#"{"ok":false,"error":"Mosh returned a null result"}"#);
    }
    // SAFETY: result is an owned NUL-terminated string returned by the C ABI.
    let output = unsafe { CStr::from_ptr(result) }.to_string_lossy().into_owned();
    // SAFETY: result is freed exactly once after copying it into a Rust String.
    unsafe { mosh_string_free(result) };
    java_string(&mut env, output)
}

#[no_mangle]
pub extern "system" fn Java_com_jusu_mosh_MoshRuntime_nativeVersion(
    mut env: JNIEnv,
    _this: JObject,
) -> jstring {
    let version = mosh_runtime_version();
    if version.is_null() {
        return java_string(&mut env, "unknown");
    }
    // SAFETY: version is an owned NUL-terminated string returned by the C ABI.
    let output = unsafe { CStr::from_ptr(version) }.to_string_lossy().into_owned();
    // SAFETY: version is freed exactly once after copying.
    unsafe { mosh_string_free(version) };
    java_string(&mut env, output)
}
