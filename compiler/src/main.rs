mod lexer;
mod parser;
mod semantic;

use std::env;
use std::fs;
use std::process::ExitCode;

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
    println!();
    println!("The lex and parse commands are development tools.");
    println!("The check command runs parsing and initial semantic analysis.");
}

fn read_source(path: &str) -> Result<String, ExitCode> {
    fs::read_to_string(path).map_err(|error| {
        eprintln!("moshc: cannot read '{path}': {error}");
        ExitCode::from(1)
    })
}

fn run_lexer(path: &str) -> ExitCode {
    let source = match read_source(path) { Ok(s) => s, Err(code) => return code };
    match lexer::tokenize(&source) {
        Ok(tokens) => {
            for token in tokens {
                println!("{:?} {:?} {}:{}-{}:{}", token.kind, token.lexeme,
                    token.span.line, token.span.column, token.span.end_line, token.span.end_column);
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
    let source = match read_source(path) { Ok(s) => s, Err(code) => return code };
    let tokens = match lexer::tokenize(&source) {
        Ok(tokens) => tokens,
        Err(error) => {
            eprintln!("moshc: {path}: {error}");
            return ExitCode::from(1);
        }
    };
    match parser::parse(&tokens) {
        Ok(program) => {
            println!("{program:#?}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("moshc: {path}: {error}");
            ExitCode::from(1)
        }
    }
}


fn run_checker(path: &str) -> ExitCode {
    let source = match read_source(path) { Ok(s) => s, Err(code) => return code };
    let tokens = match lexer::tokenize(&source) {
        Ok(tokens) => tokens,
        Err(error) => { eprintln!("moshc: {path}: {error}"); return ExitCode::from(1); }
    };
    let program = match parser::parse(&tokens) {
        Ok(program) => program,
        Err(error) => { eprintln!("moshc: {path}: {error}"); return ExitCode::from(1); }
    };
    match semantic::analyze(&program) {
        Ok(()) => { println!("{path}: semantic checks passed"); ExitCode::SUCCESS }
        Err(errors) => {
            for error in errors { eprintln!("moshc: {path}: {error}"); }
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("--help") | Some("-h") => { print_help(); ExitCode::SUCCESS }
        Some("--version") | Some("-V") => { println!("moshc {VERSION}"); ExitCode::SUCCESS }
        Some("lex") => match args.next() {
            Some(path) => run_lexer(&path),
            None => { eprintln!("moshc: 'lex' requires a source file path"); ExitCode::from(2) }
        },
        Some("parse") => match args.next() {
            Some(path) => run_parser(&path),
            None => { eprintln!("moshc: 'parse' requires a source file path"); ExitCode::from(2) }
        },
        Some("check") => match args.next() {
            Some(path) => run_checker(&path),
            None => { eprintln!("moshc: 'check' requires a source file path"); ExitCode::from(2) }
        },
        Some(other) => {
            eprintln!("moshc: unsupported command or argument: {other}");
            eprintln!("Run 'moshc --help' for usage.");
            ExitCode::from(2)
        }
        None => { print_help(); ExitCode::SUCCESS }
    }
}
