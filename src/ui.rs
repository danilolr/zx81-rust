use minifb::{Key as Host, KeyRepeat, ScaleMode, Window, WindowOptions};
use std::{
    error::Error,
    path::Path,
    time::{Duration, Instant},
};
use zx81_rust::{
    Key, Machine, ROM,
    video::{HEIGHT, WIDTH},
};

const BAR: usize = 24;
const SCREEN_HEIGHT: usize = HEIGHT + BAR * 2;
const MAP: &[(Host, Key)] = &[
    (Host::Key1, Key::Num1),
    (Host::Key2, Key::Num2),
    (Host::Key3, Key::Num3),
    (Host::Key4, Key::Num4),
    (Host::Key5, Key::Num5),
    (Host::Key6, Key::Num6),
    (Host::Key7, Key::Num7),
    (Host::Key8, Key::Num8),
    (Host::Key9, Key::Num9),
    (Host::Key0, Key::Num0),
    (Host::Q, Key::Q),
    (Host::W, Key::W),
    (Host::E, Key::E),
    (Host::R, Key::R),
    (Host::T, Key::T),
    (Host::Y, Key::Y),
    (Host::U, Key::U),
    (Host::I, Key::I),
    (Host::O, Key::O),
    (Host::P, Key::P),
    (Host::A, Key::A),
    (Host::S, Key::S),
    (Host::D, Key::D),
    (Host::F, Key::F),
    (Host::G, Key::G),
    (Host::H, Key::H),
    (Host::J, Key::J),
    (Host::K, Key::K),
    (Host::L, Key::L),
    (Host::Z, Key::Z),
    (Host::X, Key::X),
    (Host::C, Key::C),
    (Host::V, Key::V),
    (Host::B, Key::B),
    (Host::N, Key::N),
    (Host::M, Key::M),
    (Host::Period, Key::Period),
    (Host::Space, Key::Space),
    (Host::Enter, Key::Enter),
    (Host::LeftShift, Key::Shift),
    (Host::RightShift, Key::Shift),
    (Host::Escape, Key::Space),
];

fn input(window: &Window, machine: &mut Machine) {
    machine.release_keys();
    for &(host, key) in MAP {
        if window.is_key_down(host) {
            machine.set_key(key, true);
        }
    }
    for (host, key) in [
        (Host::Backspace, Key::Num0),
        (Host::Left, Key::Num5),
        (Host::Down, Key::Num6),
        (Host::Up, Key::Num7),
        (Host::Right, Key::Num8),
        (Host::Apostrophe, Key::P),
        (Host::Comma, Key::Period),
        (Host::Minus, Key::J),
        (Host::Equal, Key::L),
        (Host::Slash, Key::V),
    ] {
        if window.is_key_down(host) {
            machine.set_key(Key::Shift, true);
            machine.set_key(key, true);
        }
    }
}

pub fn run(
    machine: &mut Machine,
    program: Option<&Path>,
    frames: Option<usize>,
) -> Result<(), Box<dyn Error>> {
    let label = program
        .and_then(Path::file_name)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "BASIC".into());
    let title = format!("zx81-rust — {label}");
    let mut window = Window::new(
        &title,
        WIDTH * 2,
        SCREEN_HEIGHT * 2,
        WindowOptions {
            resize: true,
            scale_mode: ScaleMode::AspectRatioStretch,
            ..WindowOptions::default()
        },
    )?;
    window.set_target_fps(50);
    let mut surface = vec![0; WIDTH * SCREEN_HEIGHT];
    let mut paused = false;
    let mut help = false;
    let mut count = 0;
    let mut status = "P=PRINT  R=RUN  S=SAVE  J=LOAD".to_string();
    let mut status_until = Instant::now() + Duration::from_secs(8);
    while window.is_open() && frames.is_none_or(|limit| count < limit) {
        let control = window.is_key_down(Host::LeftCtrl) || window.is_key_down(Host::RightCtrl);
        if control && window.is_key_down(Host::Q) {
            break;
        }
        if window.is_key_pressed(Host::F1, KeyRepeat::No) {
            help = !help;
            machine.release_keys();
        }
        if window.is_key_pressed(Host::F5, KeyRepeat::No) {
            paused = !paused;
            machine.release_keys();
        }
        if window.is_key_pressed(Host::F9, KeyRepeat::No) {
            machine.reset();
            machine.boot()?;
            paused = false;
        }
        if window.is_active() && !paused && !help && !control {
            input(&window, machine);
        } else {
            machine.release_keys();
        }
        // Help suspends the machine so its own keyboard never receives help navigation.
        if !paused && !help {
            machine.run_frame();
        }
        count += 1;
        if let Some(message) = machine.take_status() {
            eprintln!("{message}");
            status = message;
            status_until = Instant::now() + Duration::from_secs(8);
            window.set_title(&format!("{title} — {status}"));
        }
        surface.fill(0x18212c);
        surface[BAR * WIDTH..(BAR + HEIGHT) * WIDTH].copy_from_slice(&machine.pixels());
        text(&mut surface, 8, 8, "ZX81 RUST", 0xe6edf3);
        text(
            &mut surface,
            144,
            8,
            if paused {
                "PAUSED - F5 RESUMES"
            } else {
                "F1 HELP  F5 PAUSE  F9 RESET"
            },
            0xb1c5d9,
        );
        if Instant::now() < status_until {
            text(&mut surface, 8, BAR + HEIGHT + 8, &status, 0xe6edf3);
        } else {
            text(
                &mut surface,
                8,
                BAR + HEIGHT + 8,
                "SHIFT+P QUOTES  SPACE BREAK  CTRL+Q QUIT",
                0xb1c5d9,
            );
        }
        if help {
            help_screen(&mut surface);
        }
        window.update_with_buffer(&surface, WIDTH, SCREEN_HEIGHT)?;
    }
    machine.release_keys();
    Ok(())
}

fn glyph(ch: char) -> Option<u8> {
    match ch.to_ascii_uppercase() {
        'A'..='Z' => Some(ch.to_ascii_uppercase() as u8 - b'A' + 0x26),
        '0'..='9' => Some(ch as u8 - b'0' + 0x1c),
        ' ' => Some(0),
        '.' => Some(0x1b),
        ',' => Some(0x1a),
        ':' => Some(0x0e),
        '-' => Some(0x16),
        '=' => Some(0x14),
        '+' => Some(0x15),
        '/' => Some(0x18),
        '(' => Some(0x10),
        ')' => Some(0x11),
        '"' => Some(0x0b),
        '?' => Some(0x0f),
        _ => None,
    }
}

fn text(surface: &mut [u32], x: usize, y: usize, message: &str, color: u32) {
    for (index, ch) in message.chars().enumerate() {
        let left = x + index * 8;
        if left + 8 > WIDTH {
            break;
        }
        if let Some(code) = glyph(ch) {
            for row in 0..8 {
                if y + row >= SCREEN_HEIGHT {
                    break;
                }
                let bits = ROM[0x1e00 + usize::from(code) * 8 + row];
                for col in 0..8 {
                    if bits & (0x80 >> col) != 0 {
                        surface[(y + row) * WIDTH + left + col] = color;
                    }
                }
            }
        }
    }
}

fn help_screen(surface: &mut [u32]) {
    surface[BAR * WIDTH..(BAR + HEIGHT) * WIDTH].fill(0xf1f3f5);
    let lines = [
        "ZX81 KEYBOARD - F1 RETURNS TO BASIC",
        "",
        "1 2 3 4 5 6 7 8 9 0",
        "Q W E R T Y U I O P",
        "A S D F G H J K L ENTER",
        "SHIFT Z X C V B N M . SPACE",
        "",
        "AT THE K CURSOR, A KEY INSERTS A KEYWORD:",
        "P PRINT   R RUN    S SAVE   J LOAD",
        "L LET     G GOTO   F FOR    N NEXT",
        "K LIST    I INPUT  E REM    D DIM",
        "",
        "SHIFT+P QUOTES    SHIFT+0 DELETE",
        "SHIFT+ENTER FUNCTION",
        "SHIFT+5/6/7/8 MOVE THE CURSOR",
        "SPACE OR ESCAPE SENDS BREAK",
        "",
        "EXAMPLE: 10 PRINT \"HELLO\"",
        "KEYS: 1 0 P SHIFT+P H E L L O SHIFT+P ENTER",
        "RUN: R ENTER",
        "SAVE: S SHIFT+P NAME SHIFT+P ENTER",
        "",
        "SAVE \"TEST\" WRITES TEST.p",
        "LOAD \"TEST\" READS TEST.p",
        "FILES USE THE CURRENT WORKING DIRECTORY",
        "",
        "F5 PAUSE  F9 RESET  CTRL+Q QUIT",
    ];
    for (row, line) in lines.iter().enumerate() {
        text(surface, 12, BAR + 8 + row * 10, line, 0x18212c);
    }
}
