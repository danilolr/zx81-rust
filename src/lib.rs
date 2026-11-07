//! A local ZX81 machine. The only CPU dependency is the Rust `z80emu` crate.
pub mod keyboard;
pub mod machine;
pub mod program;
pub mod video;

pub use keyboard::Key;
pub use machine::Machine;
pub use program::Program;

/// Sinclair ZX81, edition 3. Firmware attribution is in `roms/NOTICE.txt`.
pub const ROM: &[u8; 8192] = include_bytes!("../roms/zx81.edition3.rom");
