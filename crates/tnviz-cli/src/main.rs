//! `tnviz`: check, format, lay out, and draw tnv files.

use std::process::ExitCode;

const USAGE: &str = "\
usage: tnviz <command> <file> [options]
       tnviz tex <job>

commands:
  tex                   read <job>.tnvm, written by LaTeX's tnviz package,
                        and write the .tikz file of every figure in it
  check                 read a tnv file and summarize the network
  fmt                   print the network in canonical tnv
  layout [--svg FILE]   print tensor positions; --svg also draws a plain
                        debugging picture of the layout and geometry
  tikz [-o FILE]        write the figure in the runtime protocol, with
                        estimated label sizes (standard output by default)";

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
    if let [command, job] = args
        && command == "tex"
    {
        return tex(job);
    }
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
                let geom = tnviz::geometry(
                    &net,
                    &lay,
                    &tnviz::GeometryOptions::default(),
                    &tnviz::LabelSizes::new(),
                )
                .map_err(|e| Failure::Message(format!("{path}: {e}")))?;
                for w in &geom.warnings {
                    eprintln!("{path}: warning: {w}");
                }
                std::fs::write(file, tnviz::debug_svg(&geom))
                    .map_err(|e| Failure::Message(format!("tnviz: cannot write {file}: {e}")))?;
            }
        }
        ("tikz", rest) => {
            let out = match rest {
                [] => None,
                [flag, file] if flag == "-o" => Some(file),
                _ => return Err(Failure::Usage),
            };
            let code = figure(path, &net, &tnviz::GeometryOptions::default(), &tnviz::LabelSizes::new())?;
            match out {
                None => print!("{code}"),
                Some(file) => std::fs::write(file, code)
                    .map_err(|e| Failure::Message(format!("tnviz: cannot write {file}: {e}")))?,
            }
        }
        _ => return Err(Failure::Usage),
    }
    Ok(())
}

/// A network's figure in the runtime protocol; warnings go to standard
/// error.
fn figure(
    path: &str,
    net: &tnviz::Network,
    opts: &tnviz::GeometryOptions,
    sizes: &tnviz::LabelSizes,
) -> Result<String, Failure> {
    let fail = |e: tnviz::Error| Failure::Message(format!("{path}: {e}"));
    let lay = tnviz::layout(net).map_err(fail)?;
    let geom = tnviz::geometry(net, &lay, opts, sizes).map_err(fail)?;
    for w in &geom.warnings {
        eprintln!("{path}: warning: {w}");
    }
    let light = tnviz::lighting(net, &geom, opts);
    Ok(tnviz::tikz(net, &geom, &light, &tnviz::order(net, &geom), opts))
}

/// `tnviz tex <job>`: every figure of `<job>.tnvm`, next to it.  Sources
/// are relative to the `.tnvm` file's directory, like LaTeX's paths.  A
/// `.tikz` file is rewritten only when it changes, so that build tools see
/// no change after the last run.  A figure that fails does not stop the
/// others.
fn tex(job: &str) -> Result<(), Failure> {
    let job = job.strip_suffix(".tnvm").unwrap_or(job);
    let tnvm = format!("{job}.tnvm");
    let text = std::fs::read_to_string(&tnvm)
        .map_err(|e| Failure::Message(format!("tnviz: cannot read {tnvm}: {e}; run LaTeX first")))?;
    let figures = tnviz::parse_tnvm(&text).map_err(|e| Failure::Message(format!("{tnvm}:{e}")))?;
    let dir = std::path::Path::new(job).parent().unwrap_or(std::path::Path::new(""));
    let base = std::path::Path::new(job).file_name().map_or(job.into(), |n| n.to_string_lossy());
    let mut failed = 0;
    for f in &figures {
        let source = dir.join(&f.source);
        let out = dir.join(format!("{base}-{}.tikz", f.name));
        let result = (|| {
            let path = source.display().to_string();
            let src = std::fs::read_to_string(&source)
                .map_err(|e| Failure::Message(format!("tnviz: cannot read {path}: {e}")))?;
            let net = tnviz::parse(&src).map_err(|e| Failure::Message(format!("{path}:{e}")))?;
            let code = figure(&path, &net, &f.options(), &f.labels)?;
            if std::fs::read_to_string(&out).ok().as_deref() != Some(code.as_str()) {
                std::fs::write(&out, code)
                    .map_err(|e| Failure::Message(format!("tnviz: cannot write {}: {e}", out.display())))?;
                println!("{}", out.display());
            }
            Ok(())
        })();
        if let Err(Failure::Message(m)) = result {
            eprintln!("{m}");
            failed += 1;
        }
    }
    match failed {
        0 => Ok(()),
        n => Err(Failure::Message(format!("tnviz: {n} of {} figures failed", figures.len()))),
    }
}
