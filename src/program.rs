//! Native .P images contain VERSN ($4009) through E_LINE-1, not CPU state.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const START: usize = 0x4009;
pub const SYSTEM_END: usize = 0x407d;

#[derive(Clone, Debug)]
pub struct Program {
    bytes: Vec<u8>,
}

impl Program {
    pub fn read(path: &Path, ram_kb: u8) -> io::Result<Self> {
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("p"))
        {
            return Err(invalid("The program must be a .p file"));
        }
        let max = ram_end(ram_kb)? - START;
        let size = fs::metadata(path)?.len();
        if size > max as u64 {
            return Err(invalid("The program exceeds the configured RAM"));
        }
        Self::from_bytes(fs::read(path)?, ram_kb)
    }

    pub fn from_bytes(mut bytes: Vec<u8>, ram_kb: u8) -> io::Result<Self> {
        let ram_end = ram_end(ram_kb)?;
        if bytes.len() < SYSTEM_END - START || bytes.len() > ram_end - START {
            return Err(invalid("Invalid .p file size for the configured RAM"));
        }
        let word = |address: usize| {
            u16::from_le_bytes([bytes[address - START], bytes[address - START + 1]]) as usize
        };
        let end = word(0x4014);
        let display = word(0x400c);
        let vars = word(0x4010);
        if end > START + bytes.len()
            || end + 4 > ram_end
            || !(SYSTEM_END..end).contains(&display)
            || vars <= display
            || vars >= end
        {
            return Err(invalid(
                "Invalid .p file: inconsistent program, display, or variable pointers",
            ));
        }
        // Some tape tools append the edit-line terminator. It is outside .P's
        // memory range and must not overwrite the fresh editor workspace.
        bytes.truncate(end - START);
        Ok(Self { bytes })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

pub(crate) fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn ram_end(ram_kb: u8) -> io::Result<usize> {
    if ![1, 2, 4, 8, 16].contains(&ram_kb) {
        return Err(invalid("RAM: use 1, 2, 4, 8, or 16 KiB"));
    }
    Ok(0x4000 + usize::from(ram_kb) * 1024)
}

/// Resolve a BASIC tape name relative to the working directory, with .p suffix.
pub fn file_path(directory: &Path, name: &str) -> io::Result<PathBuf> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':'])
        || name.chars().any(char::is_control)
    {
        return Err(invalid("Use a simple file name, such as SAVE \"TEST\""));
    }
    let mut path = directory.join(name);
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("p"))
    {
        path.as_mut_os_string().push(".p");
    }
    Ok(path)
}

/// Conversion for names passed by the Sinclair ROM to its tape routines.
pub(crate) fn decode_character(code: u8) -> Option<char> {
    match code & 0x7f {
        0 => Some(' '),
        0x0b => Some('"'),
        0x0c => Some('£'),
        0x0d => Some('$'),
        0x0e => Some(':'),
        0x0f => Some('?'),
        0x10 => Some('('),
        0x11 => Some(')'),
        0x12 => Some('>'),
        0x13 => Some('<'),
        0x14 => Some('='),
        0x15 => Some('+'),
        0x16 => Some('-'),
        0x17 => Some('*'),
        0x18 => Some('/'),
        0x19 => Some(';'),
        0x1a => Some(','),
        0x1b => Some('.'),
        c @ 0x1c..=0x25 => Some((b'0' + c - 0x1c) as char),
        c @ 0x26..=0x3f => Some((b'A' + c - 0x26) as char),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_image() -> Vec<u8> {
        let display = SYSTEM_END;
        let vars = display + 25;
        let end = vars + 1;
        let mut bytes = vec![0; end - START];
        for (address, value) in [(0x400c, display), (0x4010, vars), (0x4014, end)] {
            bytes[address - START..address - START + 2]
                .copy_from_slice(&(value as u16).to_le_bytes());
        }
        bytes[display - START..vars - START].fill(0x76);
        bytes[vars - START] = 0x80;
        bytes
    }

    #[test]
    fn validates_pointers_size_ram_and_truncated_images_before_loading() {
        let good = valid_image();
        assert!(Program::from_bytes(good.clone(), 1).is_ok());
        assert!(Program::from_bytes(good.clone(), 0).is_err());
        assert!(Program::from_bytes(vec![], 16).is_err());
        assert!(Program::from_bytes(vec![0; 65536], 16).is_err());
        for address in [0x400c, 0x4010, 0x4014] {
            let mut bad = good.clone();
            bad[address - START..address - START + 2].fill(0xff);
            assert!(Program::from_bytes(bad, 16).is_err());
        }
        let mut short = good.clone();
        short.pop();
        assert!(Program::from_bytes(short, 16).is_err());
        let mut trailing = good.clone();
        trailing.push(0x76);
        assert_eq!(Program::from_bytes(trailing, 16).unwrap().bytes(), good);
    }

    #[test]
    fn resolves_basic_names_without_double_extensions_or_directory_traversal() {
        let root = Path::new("/tmp");
        assert_eq!(file_path(root, "TEST").unwrap(), root.join("TEST.p"));
        assert_eq!(file_path(root, "TEST.P").unwrap(), root.join("TEST.P"));
        for name in [
            "", "  ", ".", "..", "../OTHER", "A/B", "A\\B", "/tmp/A", "A\nB",
        ] {
            assert!(file_path(root, name).is_err());
        }
        assert_eq!(decode_character(0xa6), Some('A'));
    }
}
