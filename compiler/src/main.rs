pub mod layout;
pub mod ownership;
mod lexer;
mod llvm;
mod mir;
mod parser;
mod semantic;
mod typecheck;

use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    println!("moshc {VERSION}");
    println!("The Mosh programming language compiler (early development)");
    println!();
    println!("USAGE:");
    println!("    moshc --help");
    println!("    moshc --version");
    println!("    moshc lex <source.mosh>");
    println!("    moshc parse <source.mosh>");
    println!("    moshc check <source.mosh>");
    println!("    moshc mir <source.mosh>");
    println!("    moshc build <source.mosh> [-o <executable>]");
    println!();
    println!("The lex and parse commands are development tools.");
    println!("The check command runs parsing, name resolution, and static type checking.");
    println!("The mir command lowers a checked program to Mosh IR.");
    println!(
        "The build command generates LLVM IR and invokes clang to create a native executable."
    );
}

fn read_source(path: &str) -> Result<String, ExitCode> {
    fs::read_to_string(path).map_err(|error| {
        eprintln!("moshc: cannot read '{path}': {error}");
        ExitCode::from(1)
    })
}

fn parse_program(path: &str, tokens: &[lexer::Token]) -> Option<parser::Program> {
    match parser::parse_recovering(tokens) {
        Ok(output) if output.errors.is_empty() => Some(output.program),
        Ok(output) => {
            for error in output.errors {
                eprintln!("moshc: {path}: {error}");
            }
            None
        }
        Err(error) => {
            eprintln!("moshc: {path}: {error}");
            None
        }
    }
}

fn run_lexer(path: &str) -> ExitCode {
    let source = match read_source(path) {
        Ok(s) => s,
        Err(code) => return code,
    };
    match lexer::tokenize(&source) {
        Ok(tokens) => {
            for token in tokens {
                println!(
                    "{:?} {:?} {}:{}-{}:{}",
                    token.kind,
                    token.lexeme,
                    token.span.line,
                    token.span.column,
                    token.span.end_line,
                    token.span.end_column
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("moshc: {path}: {error}");
            ExitCode::from(1)
        }
    }
}

fn run_parser(path: &str) -> ExitCode {
    let source = match read_source(path) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let tokens = match lexer::tokenize(&source) {
        Ok(tokens) => tokens,
        Err(error) => {
            eprintln!("moshc: {path}: {error}");
            return ExitCode::from(1);
        }
    };
    match parse_program(path, &tokens) {
        Some(program) => {
            println!("{program:#?}");
            ExitCode::SUCCESS
        }
        None => ExitCode::from(1),
    }
}

fn run_checker(path: &str) -> ExitCode {
    let source = match read_source(path) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let tokens = match lexer::tokenize(&source) {
        Ok(tokens) => tokens,
        Err(error) => {
            eprintln!("moshc: {path}: {error}");
            return ExitCode::from(1);
        }
    };
    let Some(program) = parse_program(path, &tokens) else {
        return ExitCode::from(1);
    };
    if let Err(errors) = semantic::analyze(&program) {
        for error in errors {
            eprintln!("moshc: {path}: {error}");
        }
        return ExitCode::from(1);
    }
    match typecheck::check(&program) {
        Ok(()) => {
            println!("{path}: semantic and type checks passed");
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for error in errors {
                eprintln!("moshc: {path}: {error}");
            }
            ExitCode::from(1)
        }
    }
}

fn run_mir(path: &str) -> ExitCode {
    let source = match read_source(path) {
        Ok(s) => s,
        Err(code) => return code,
    };
    let tokens = match lexer::tokenize(&source) {
        Ok(tokens) => tokens,
        Err(error) => {
            eprintln!("moshc: {path}: {error}");
            return ExitCode::from(1);
        }
    };
    let Some(program) = parse_program(path, &tokens) else {
        return ExitCode::from(1);
    };
    if let Err(errors) = semantic::analyze(&program) {
        for error in errors {
            eprintln!("moshc: {path}: {error}");
        }
        return ExitCode::from(1);
    }
    if let Err(errors) = typecheck::check(&program) {
        for error in errors {
            eprintln!("moshc: {path}: {error}");
        }
        return ExitCode::from(1);
    }
    match mir::lower(&program) {
        Ok(module) => {
            println!("{module:#?}");
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for error in errors {
                eprintln!("moshc: {path}: {error}");
            }
            ExitCode::from(1)
        }
    }
}

fn run_build(path: &str, output: Option<&str>) -> ExitCode {
    let source = match read_source(path) {
        Ok(source) => source,
        Err(code) => return code,
    };
    let tokens = match lexer::tokenize(&source) {
        Ok(tokens) => tokens,
        Err(error) => {
            eprintln!("moshc: {path}: {error}");
            return ExitCode::from(1);
        }
    };
    let Some(program) = parse_program(path, &tokens) else {
        return ExitCode::from(1);
    };
    if let Err(errors) = semantic::analyze(&program) {
        for error in errors {
            eprintln!("moshc: {path}: {error}");
        }
        return ExitCode::from(1);
    }
    if let Err(errors) = typecheck::check(&program) {
        for error in errors {
            eprintln!("moshc: {path}: {error}");
        }
        return ExitCode::from(1);
    }
    let module = match mir::lower(&program) {
        Ok(module) => module,
        Err(errors) => {
            for error in errors {
                eprintln!("moshc: {path}: {error}");
            }
            return ExitCode::from(1);
        }
    };
    let ir = match llvm::generate(&module) {
        Ok(ir) => ir,
        Err(errors) => {
            for error in errors {
                eprintln!("moshc: {path}: {error}");
            }
            return ExitCode::from(1);
        }
    };

    let source_name = Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("mosh");
    let ir_path =
        std::env::temp_dir().join(format!("moshc-{}-{source_name}.ll", std::process::id()));
    if let Err(error) = fs::write(&ir_path, ir) {
        eprintln!("moshc: cannot write temporary LLVM IR: {error}");
        return ExitCode::from(1);
    }
    let default_output = Path::new(path).with_extension("");
    let output_path = output
        .map(Path::new)
        .map(Path::to_path_buf)
        .unwrap_or(default_output);
    let result = Command::new("clang")
        .arg("-x")
        .arg("ir")
        .arg("-O0")
        .arg(&ir_path)
        .arg("-o")
        .arg(&output_path)
        .output();
    let _ = fs::remove_file(&ir_path);

    match result {
        Ok(result) if result.status.success() => {
            println!("Built native executable: {}", output_path.display());
            ExitCode::SUCCESS
        }
        Ok(result) => {
            eprintln!("moshc: clang failed while building '{path}'");
            if !result.stderr.is_empty() {
                eprint!("{}", String::from_utf8_lossy(&result.stderr));
            }
            ExitCode::from(1)
        }
        Err(error) => {
            eprintln!("moshc: could not run clang: {error}");
            eprintln!("Install LLVM/Clang and ensure 'clang' is available on PATH.");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("--help") | Some("-h") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some("--version") | Some("-V") => {
            println!("moshc {VERSION}");
            ExitCode::SUCCESS
        }
        Some("lex") => match args.next() {
            Some(path) => run_lexer(&path),
            None => {
                eprintln!("moshc: 'lex' requires a source file path");
                ExitCode::from(2)
            }
        },
        Some("parse") => match args.next() {
            Some(path) => run_parser(&path),
            None => {
                eprintln!("moshc: 'parse' requires a source file path");
                ExitCode::from(2)
            }
        },
        Some("mir") => match args.next() {
            Some(path) => run_mir(&path),
            None => {
                eprintln!("moshc: mir requires a source file path");
                ExitCode::from(2)
            }
        },
        Some("check") => match args.next() {
            Some(path) => run_checker(&path),
            None => {
                eprintln!("moshc: 'check' requires a source file path");
                ExitCode::from(2)
            }
        },
        Some("build") => {
            let Some(path) = args.next() else {
                eprintln!("moshc: 'build' requires a source file path");
                return ExitCode::from(2);
            };
            let mut output = None;
            while let Some(argument) = args.next() {
                if argument == "-o" {
                    if output.is_some() {
                        eprintln!("moshc: '-o' may only be supplied once");
                        return ExitCode::from(2);
                    }
                    output = args.next();
                    if output.is_none() {
                        eprintln!("moshc: '-o' requires an output path");
                        return ExitCode::from(2);
                    }
                } else {
                    eprintln!("moshc: unexpected build argument '{argument}'");
                    return ExitCode::from(2);
                }
            }
            run_build(&path, output.as_deref())
        }
        Some(other) => {
            eprintln!("moshc: unsupported command or argument: {other}");
            eprintln!("Run 'moshc --help' for usage.");
            ExitCode::from(2)
        }
        None => {
            print_help();
            ExitCode::SUCCESS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::run_build;

    fn assert_build_rejects(name: &str, source_text: &str) {
        let base = std::env::temp_dir().join(format!("moshc-cli-{name}-{}", std::process::id()));
        let source = base.with_extension("mosh");
        std::fs::write(&source, source_text).unwrap();

        let status = run_build(source.to_str().unwrap(), None);
        let _ = std::fs::remove_file(&source);
        assert_eq!(status, std::process::ExitCode::from(1));
    }

    #[test]
    fn build_rejects_module_level_global_bindings() {
        assert_build_rejects("global", "let value: Int = 42\n");
    }

    #[test]
    fn build_rejects_nested_function_declarations() {
        assert_build_rejects(
            "nested",
            "function main():\n    function helper():\n        print(\"nested\")\n",
        );
    }

    #[test]
    fn build_rejects_for_loops_until_iteration_is_implemented() {
        assert_build_rejects(
            "for",
            "function main():\n    for item in 1:\n        print(item)\n",
        );
    }

    #[test]
    fn build_command_creates_a_runnable_native_executable() {
        let base = std::env::temp_dir().join(format!("moshc-cli-smoke-{}", std::process::id()));
        let source = base.with_extension("mosh");
        let executable = base.with_extension("bin");
        std::fs::write(&source, "function main() -> Int:\n    return 42\n").unwrap();

        let source_path = source.to_str().unwrap();
        let executable_path = executable.to_str().unwrap();
        let status = run_build(source_path, Some(executable_path));
        assert_eq!(status, std::process::ExitCode::SUCCESS);

        let result = std::process::Command::new(&executable).output().unwrap();
        let _ = std::fs::remove_file(&source);
        let _ = std::fs::remove_file(&executable);
        assert_eq!(result.status.code(), Some(42));
    }
}
