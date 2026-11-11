mod lexer;

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
    println!();
    println!("The lex command prints the provisional token stream.");
}

fn run_lexer(path: &str) -> ExitCode {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("moshc: cannot read '{path}': {error}");
            return ExitCode::from(1);
        }
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
