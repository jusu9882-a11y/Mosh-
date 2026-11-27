use std::sync::Arc;

use moshc::device::UnsupportedBackend;
use moshc::{lexer, parser, Interpreter};

#[test]
fn public_library_api_executes_mosh_source() {
    let tokens = lexer::tokenize("let answer = 40 + 2\nanswer\n").unwrap();
    let program = parser::parse(&tokens).unwrap();
    let mut interpreter = Interpreter::new();
    assert_eq!(interpreter.execute(&program).unwrap(), moshc::Value::Integer(42));
}

#[test]
fn public_library_api_accepts_an_injected_device_backend() {
    let tokens = lexer::tokenize("device.camera.is_supported()\n").unwrap();
    let program = parser::parse(&tokens).unwrap();
    let mut interpreter = Interpreter::with_device_backend(Arc::new(UnsupportedBackend));
    assert_eq!(interpreter.execute(&program).unwrap(), moshc::Value::Boolean(false));
}
