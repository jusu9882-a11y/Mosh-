//! Public Mosh interpreter runtime for embedding in native host applications.
//!
//! Device access is supplied by the host through `DeviceBackend`; the interpreter
//! itself does not assume that a platform or hardware resource is available.

pub mod android_backend;
pub mod device;
pub mod ffi;
pub mod interpreter;
pub mod jni_bridge;
pub mod lexer;
pub mod module_registry;
pub mod parser;
pub mod semantic;
pub mod typecheck;

pub use interpreter::{Interpreter, RuntimeError, Value};
