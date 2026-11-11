use std::env;
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    println!("moshc {VERSION}");
    println!("The Mosh programming language compiler (development skeleton)");
    println!();
    println!("USAGE:");
    println!("    moshc --help");
    println!("    moshc --version");
    println!();
    println!("Compiler functionality is not implemented yet.");
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
        Some(other) => {
            eprintln!("moshc: unsupported argument: {other}");
            eprintln!("Run 'moshc --help' for usage.");
            ExitCode::from(2)
        }
        None => {
            print_help();
            ExitCode::SUCCESS
        }
    }
}
