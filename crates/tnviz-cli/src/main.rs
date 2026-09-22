//! `tnviz`: check and format tnv files.

use std::process::ExitCode;

const USAGE: &str = "\
usage: tnviz <command> <file>

commands:
  check   read a tnv file and summarize the network
  fmt     print the network in canonical tnv";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [command, path] = args.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("tnviz: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let net = match tnviz::parse(&src) {
        Ok(net) => net,
        Err(e) => {
            eprintln!("{path}:{e}");
            return ExitCode::FAILURE;
        }
    };
    match command.as_str() {
        "check" => {
            println!(
                "{path}: {} tensors, {} bonds, {} open legs",
                net.tensors().len(),
                net.bonds().count(),
                net.open_legs().count()
            );
        }
        "fmt" => print!("{}", tnviz::to_tnv(&net)),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
