mod interpreter;
mod lexer;
mod parser;
mod semantic;
mod typecheck;

use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    println!("Mosh {VERSION} — interpreted programming language");
    println!();
    println!("USAGE:");
    println!("    mosh [repl]");
    println!("    mosh run <source.mosh>");
    println!("    mosh check <source.mosh>");
    println!("    mosh lex <source.mosh>");
    println!("    mosh parse <source.mosh>");
    println!("    mosh --version");
    println!();
    println!("Mosh runs source files directly in its interpreter.");
    println!("No native compiler or executable-generation command is provided.");
    println!("The check command runs the current semantic and type diagnostics.");
}

fn read_source(path: &str) -> Result<String, ExitCode> {
    fs::read_to_string(path).map_err(|error| {
        eprintln!("mosh: cannot read '{path}': {error}");
        ExitCode::from(1)
    })
}

fn parse_program(path: &str, tokens: &[lexer::Token]) -> Option<parser::Program> {
    match parser::parse_recovering(tokens) {
        Ok(output) if output.errors.is_empty() => Some(output.program),
        Ok(output) => {
            for error in output.errors { eprintln!("mosh: {path}: {error}"); }
            None
        }
        Err(error) => { eprintln!("mosh: {path}: {error}"); None }
    }
}

fn parse_source(path: &str, source: &str) -> Option<parser::Program> {
    let tokens = match lexer::tokenize(source) {
        Ok(tokens) => tokens,
        Err(error) => { eprintln!("mosh: {path}: {error}"); return None; }
    };
    parse_program(path, &tokens)
}

fn run_file(path: &str) -> ExitCode {
    if !path.ends_with(".mosh") {
        eprintln!("mosh: expected a .mosh source file");
        return ExitCode::from(2);
    }
    let source = match read_source(path) { Ok(s) => s, Err(code) => return code };
    let Some(program) = parse_source(path, &source) else { return ExitCode::from(1); };
    let mut interpreter = interpreter::Interpreter::new();
    interpreter.set_source_path(path);
    match interpreter.execute(&program) {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => { eprintln!("mosh: {path}: runtime error: {error}"); ExitCode::from(1) }
    }
}

fn run_repl() -> ExitCode {
    println!("Mosh {VERSION} interactive interpreter");
    println!("Type :help for commands; :quit or :q to exit.");
    let stdin = io::stdin();
    let mut interpreter = interpreter::Interpreter::new();
    interpreter.set_echo(true);
    let mut line = String::new();

    loop {
        print!(">>> ");
        if io::stdout().flush().is_err() { return ExitCode::from(1); }
        line.clear();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => { eprintln!("mosh: input error: {error}"); return ExitCode::from(1); }
        }

        let first = line.trim_end_matches(['\r', '\n']);
        if first == ":quit" || first == ":q" { break; }
        if first == ":help" {
            println!("Enter Mosh expressions or statements.");
            println!("For if/else, while, for, and function blocks, finish with a blank line.");
            println!("Commands: :help, :quit, :q");
            continue;
        }
        if first.trim().is_empty() { continue; }

        let mut source = first.to_owned();
        if first.trim_end().ends_with(':') {
            loop {
                print!("... ");
                if io::stdout().flush().is_err() { return ExitCode::from(1); }
                line.clear();
                match stdin.lock().read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(error) => { eprintln!("mosh: input error: {error}"); return ExitCode::from(1); }
                }
                let continuation = line.trim_end_matches(['\r', '\n']);
                if continuation.trim().is_empty() { break; }
                source.push('\n');
                source.push_str(continuation);
            }
        }

        let Some(program) = parse_source("<repl>", &source) else { continue; };
        match interpreter.execute(&program) {
            Ok(_) => {}
            Err(error) => eprintln!("mosh: runtime error: {error}"),
        }
    }
    println!("Goodbye.");
    ExitCode::SUCCESS
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
        Err(error) => { eprintln!("mosh: {path}: {error}"); ExitCode::from(1) }
    }
}

fn run_parser(path: &str) -> ExitCode {
    let source = match read_source(path) { Ok(s) => s, Err(code) => return code };
    match parse_source(path, &source) {
        Some(program) => { println!("{program:#?}"); ExitCode::SUCCESS }
        None => ExitCode::from(1),
    }
}

fn run_checker(path: &str) -> ExitCode {
    let source = match read_source(path) { Ok(s) => s, Err(code) => return code };
    let Some(program) = parse_source(path, &source) else { return ExitCode::from(1); };
    if let Err(errors) = semantic::analyze(&program) {
        for error in errors { eprintln!("mosh: {path}: {error}"); }
        return ExitCode::from(1);
    }
    match typecheck::check(&program) {
        Ok(()) => { println!("{path}: semantic and type checks passed"); ExitCode::SUCCESS }
        Err(errors) => {
            for error in errors { eprintln!("mosh: {path}: {error}"); }
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("--help") | Some("-h") => { print_help(); ExitCode::SUCCESS }
        Some("--version") | Some("-V") => { println!("mosh {VERSION}"); ExitCode::SUCCESS }
        Some("run") => match args.next() {
            Some(path) => run_file(&path),
            None => { eprintln!("mosh: 'run' requires a .mosh file"); ExitCode::from(2) }
        },
        Some("repl") => run_repl(),
        Some("lex") => match args.next() {
            Some(path) => run_lexer(&path),
            None => { eprintln!("mosh: 'lex' requires a source file"); ExitCode::from(2) }
        },
        Some("parse") => match args.next() {
            Some(path) => run_parser(&path),
            None => { eprintln!("mosh: 'parse' requires a source file"); ExitCode::from(2) }
        },
        Some("check") => match args.next() {
            Some(path) => run_checker(&path),
            None => { eprintln!("mosh: 'check' requires a source file"); ExitCode::from(2) }
        },
        Some(other) => {
            eprintln!("mosh: unsupported command or argument: {other}");
            eprintln!("Run 'mosh --help' for usage.");
            ExitCode::from(2)
        }
        None => run_repl(),
    }
}
