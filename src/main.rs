mod ui;

use std::{env, ffi::OsString, path::PathBuf, process::ExitCode};
use zx81_rust::{Machine, Program, video};

const HELP: &str = "zx81-rust — standalone ZX81 emulator in Rust

Usage: zx81-rust [OPTIONS] [program.p]

Without a file, starts BASIC. With a .p file, loads it and waits for RUN (R, Enter).
SAVE \"TEST\" writes TEST.p in the working directory; LOAD \"TEST\" reads it.

  --ram N             RAM in KiB: 1, 2, 4, 8, or 16 (default: 16)
  --wrx               Enable WRX graphics
  --headless          Run without a window for diagnostics
  --frames N          Exit after N execution frames (headless default: 300)
  --screenshot FILE   Save the final image as a PPM file on exit
  -h, --help          Show this help
  -V, --version       Show the version

Window: F1 keyboard help, F5 pause, F9 reset, Ctrl+Q quit.
ZX81 keyboard: P=PRINT, R=RUN, S=SAVE, J=LOAD, Shift+P=quotes.
Backspace deletes; arrows move the cursor; Escape or Space sends BREAK.
";

struct Options {
    program: Option<PathBuf>,
    ram: u8,
    wrx: bool,
    headless: bool,
    frames: Option<usize>,
    screenshot: Option<PathBuf>,
}

fn options(arguments: impl IntoIterator<Item = OsString>) -> Result<Option<Options>, String> {
    let mut result = Options {
        program: None,
        ram: 16,
        wrx: false,
        headless: false,
        frames: None,
        screenshot: None,
    };
    let mut args = arguments.into_iter();
    let mut positional = false;
    while let Some(arg) = args.next() {
        if !positional {
            match arg.to_str() {
                Some("-h" | "--help") => {
                    print!("{HELP}");
                    return Ok(None);
                }
                Some("-V" | "--version") => {
                    println!("zx81-rust {}", env!("CARGO_PKG_VERSION"));
                    return Ok(None);
                }
                Some("--") => {
                    positional = true;
                    continue;
                }
                Some("--wrx") => {
                    result.wrx = true;
                    continue;
                }
                Some("--headless") => {
                    result.headless = true;
                    continue;
                }
                Some("--ram") => {
                    result.ram = args
                        .next()
                        .and_then(|s| s.to_str()?.parse().ok())
                        .filter(|r| [1, 2, 4, 8, 16].contains(r))
                        .ok_or("--ram requires 1, 2, 4, 8, or 16")?;
                    continue;
                }
                Some("--frames") => {
                    result.frames = Some(
                        args.next()
                            .and_then(|s| s.to_str()?.parse().ok())
                            .filter(|&n| n > 0)
                            .ok_or("--frames requires a positive integer")?,
                    );
                    continue;
                }
                Some("--screenshot") => {
                    let path: PathBuf = args
                        .next()
                        .ok_or("--screenshot requires a .ppm path")?
                        .into();
                    if !path
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("ppm"))
                    {
                        return Err("--screenshot requires a file with a .ppm extension".into());
                    }
                    result.screenshot = Some(path);
                    continue;
                }
                Some(s) if s.starts_with('-') => {
                    return Err(format!("Unknown option: {s}. Use --help."));
                }
                _ => (),
            }
        }
        if result.program.replace(arg.into()).is_some() {
            return Err("Specify only one .p file".into());
        }
    }
    Ok(Some(result))
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let Some(options) = options(env::args_os().skip(1))? else {
        return Ok(());
    };
    // Validate inputs before booting or opening a window.
    let program = options
        .program
        .as_deref()
        .map(|p| Program::read(p, options.ram))
        .transpose()?;
    let mut machine = Machine::new(options.ram, env::current_dir()?)?;
    machine.set_wrx(options.wrx);
    if let Some(program) = program {
        machine.load_program(program)?;
    } else {
        machine.boot()?;
    }
    if options.headless {
        let start = std::time::Instant::now();
        let frames = options.frames.unwrap_or(300);
        machine.run_frames(frames);
        println!(
            "zx81-rust: RAM={} KiB, frames={}, video fetches={}, time={:.3}s",
            machine.ram_kb(),
            machine.frames(),
            machine.video_fetches(),
            start.elapsed().as_secs_f64()
        );
        if let Some(status) = machine.take_status() {
            println!("{status}");
        }
    } else {
        ui::run(&mut machine, options.program.as_deref(), options.frames)?;
    }
    if let Some(path) = options.screenshot {
        video::write_ppm(&path, &machine.pixels())?;
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("zx81-rust: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<Option<Options>, String> {
        options(args.iter().map(OsString::from))
    }
    #[test]
    fn parses_optional_program_and_rejects_invalid_options() {
        assert!(parse(&[]).unwrap().unwrap().program.is_none());
        let o = parse(&["--ram", "1", "program.p", "--headless", "--frames", "10"])
            .unwrap()
            .unwrap();
        assert_eq!(o.ram, 1);
        assert_eq!(o.frames, Some(10));
        assert!(o.headless);
        assert_eq!(o.program, Some("program.p".into()));
        for args in [
            &["--ram", "0"][..],
            &["--ram"],
            &["--frames", "0"],
            &["--frames", "-1"],
            &["--screenshot"],
            &["a.p", "b.p"],
            &["--unknown"],
        ] {
            assert!(parse(args).is_err());
        }
        assert_eq!(
            parse(&["--", "-program.p"]).unwrap().unwrap().program,
            Some("-program.p".into())
        );
    }
}
