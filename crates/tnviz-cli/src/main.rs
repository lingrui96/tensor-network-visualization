//! `tnviz`: check, format, and lay out tnv files.

use std::process::ExitCode;

const USAGE: &str = "\
usage: tnviz <command> <file> [options]

commands:
  check                 read a tnv file and summarize the network
  fmt                   print the network in canonical tnv
  layout [--svg FILE]   print tensor positions; --svg also draws a plain
                        debugging picture of the layout";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage) => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Message(m)) => {
            eprintln!("{m}");
            ExitCode::FAILURE
        }
    }
}

enum Failure {
    Usage,
    Message(String),
}

fn run(args: &[String]) -> Result<(), Failure> {
    let (command, path, rest) = match args {
        [command, path, rest @ ..] => (command.as_str(), path, rest),
        _ => return Err(Failure::Usage),
    };
    let src = std::fs::read_to_string(path)
        .map_err(|e| Failure::Message(format!("tnviz: cannot read {path}: {e}")))?;
    let net = tnviz::parse(&src).map_err(|e| Failure::Message(format!("{path}:{e}")))?;
    match (command, rest) {
        ("check", []) => {
            println!(
                "{path}: {} tensors, {} bonds, {} open legs",
                net.tensors().len(),
                net.bonds().count(),
                net.open_legs().count()
            );
        }
        ("fmt", []) => print!("{}", tnviz::to_tnv(&net)),
        ("layout", rest) => {
            let svg = match rest {
                [] => None,
                [flag, file] if flag == "--svg" => Some(file),
                _ => return Err(Failure::Usage),
            };
            let lay = tnviz::layout(&net).map_err(|e| Failure::Message(format!("{path}: {e}")))?;
            for (id, t) in net.tensors() {
                let p = lay.pos(id);
                println!("{:<12} {:>8.3} {:>8.3} {:>8.3}", t.name.to_string(), p.x, p.y, p.z);
            }
            if let Some(file) = svg {
                std::fs::write(file, tnviz::debug_svg(&net, &lay))
                    .map_err(|e| Failure::Message(format!("tnviz: cannot write {file}: {e}")))?;
            }
        }
        _ => return Err(Failure::Usage),
    }
    Ok(())
}
