use crate::{
    Key, Program, ROM,
    keyboard::Keyboard,
    program::{self, START},
    video::{self, Ula},
};
use std::{
    cell::RefCell,
    fs,
    io::{self, Write},
    num::{NonZeroU8, NonZeroU16},
    path::PathBuf,
    rc::Rc,
};
use z80emu::{Clock as CpuClock, Cpu, CpuDebugFn, CpuFlags, Io, Memory, Prefix, StkReg16, Z80NMOS};

struct Bus {
    ram: Vec<u8>,
    keyboard: Keyboard,
    ula: Rc<RefCell<Ula>>,
    wrx: bool,
}

impl Bus {
    fn byte(&self, address: u16) -> u8 {
        let address = usize::from(address & 0x7fff);
        if address < 0x4000 {
            ROM[address & 0x1fff]
        } else {
            self.ram[(address - 0x4000) & (self.ram.len() - 1)]
        }
    }
    fn put(&mut self, address: u16, value: u8) {
        let address = usize::from(address & 0x7fff);
        if address >= 0x4000 {
            let index = (address - 0x4000) & (self.ram.len() - 1);
            self.ram[index] = value;
        }
    }
    fn word(&self, address: u16) -> u16 {
        u16::from_le_bytes([self.byte(address), self.byte(address.wrapping_add(1))])
    }
    fn put_word(&mut self, address: u16, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.put(address, lo);
        self.put(address + 1, hi);
    }
}

impl Memory for Bus {
    type Timestamp = u64;
    fn read_debug(&self, address: u16) -> u8 {
        self.byte(address)
    }
    fn write_mem(&mut self, address: u16, value: u8, _ts: u64) {
        self.put(address, value);
    }
    fn read_opcode(&mut self, pc: u16, ir: u16, _ts: u64) -> u8 {
        let code = self.byte(pc);
        let mut ula = self.ula.borrow_mut();
        ula.irq = ir & 0x40 == 0;
        if pc & 0x8000 != 0 && code & 0x40 == 0 {
            let address = if self.wrx && ir & 0xc000 != 0 {
                ir
            } else {
                (ir & 0xfe00) | (u16::from(code & 0x3f) << 3) | u16::from(ula.char_row)
            };
            let dots = self.byte(address) ^ if code & 0x80 != 0 { 0xff } else { 0 };
            ula.shift(dots);
            0 // the ULA replaces the character with a NOP on the CPU's data bus
        } else {
            code
        }
    }
}

impl Io for Bus {
    type Timestamp = u64;
    type WrIoBreak = ();
    type RetiBreak = ();
    fn read_io(&mut self, port: u16, _ts: u64) -> (u8, Option<NonZeroU16>) {
        if port & 1 == 0 {
            self.ula.borrow_mut().start_vsync();
            // PAL bit 6 is high; EAR is low with no virtual tape connected.
            (self.keyboard.read(port) & 0x7f, None)
        } else {
            (0xff, None)
        }
    }
    fn write_io(&mut self, port: u16, _value: u8, _ts: u64) -> (Option<()>, Option<NonZeroU16>) {
        self.ula.borrow_mut().output(port);
        (None, None)
    }
    fn is_irq(&mut self, _ts: u64) -> bool {
        self.ula.borrow().irq
    }
}

struct Clock {
    ula: Rc<RefCell<Ula>>,
    halted: bool,
}

impl Clock {
    fn cycle(&mut self, address: u16, count: u64) -> u64 {
        let mut ula = self.ula.borrow_mut();
        ula.advance(1); // WAIT is sampled in the second T-state of a bus cycle.
        if !self.halted && ula.nmi_enabled && ula.hsync() {
            let wait = u64::from(32 - ula.line_t);
            ula.advance(wait);
        }
        ula.advance(count - 1);
        ula.irq = address & 0x40 == 0;
        ula.ticks
    }
}

impl CpuClock for Clock {
    type Limit = u64;
    type Timestamp = u64;
    fn is_past_limit(&self, limit: u64) -> bool {
        self.as_timestamp() >= limit
    }
    fn add_irq(&mut self, _pc: u16) -> u64 {
        let mut ula = self.ula.borrow_mut();
        // Interrupt acknowledge resets the horizontal divider.
        ula.line_t = 0;
        ula.advance(6);
        ula.ticks
    }
    fn add_no_mreq(&mut self, address: u16, count: NonZeroU8) {
        let mut ula = self.ula.borrow_mut();
        ula.advance(u64::from(count.get()));
        ula.irq = address & 0x40 == 0;
    }
    fn add_m1(&mut self, address: u16) -> u64 {
        self.cycle(address, 4)
    }
    fn add_mreq(&mut self, address: u16) -> u64 {
        self.cycle(address, 3)
    }
    fn add_io(&mut self, address: u16) -> u64 {
        self.cycle(address, 4)
    }
    fn add_wait_states(&mut self, _address: u16, count: NonZeroU16) {
        self.ula.borrow_mut().advance(u64::from(count.get()));
    }
    fn as_timestamp(&self) -> u64 {
        self.ula.borrow().ticks
    }
}

/// A single ZX81. No reference emulator, network service or external worker is used.
pub struct Machine {
    cpu: Z80NMOS,
    bus: Bus,
    clock: Clock,
    directory: PathBuf,
    mounted: Option<Program>,
    next_frame: u64,
    status: Option<String>,
    load_count: u64,
    save_count: u64,
    editor_seen: bool,
}

impl Machine {
    pub fn new(ram_kb: u8, directory: impl Into<PathBuf>) -> io::Result<Self> {
        if ![1, 2, 4, 8, 16].contains(&ram_kb) {
            return Err(program::invalid("RAM: use 1, 2, 4, 8, or 16 KiB"));
        }
        let ula = Rc::new(RefCell::new(Ula::default()));
        let mut cpu = Z80NMOS::default();
        cpu.reset();
        Ok(Self {
            cpu,
            bus: Bus {
                ram: vec![0; usize::from(ram_kb) * 1024],
                keyboard: Keyboard::default(),
                ula: ula.clone(),
                wrx: false,
            },
            clock: Clock { ula, halted: false },
            directory: directory.into(),
            mounted: None,
            next_frame: 0,
            status: None,
            load_count: 0,
            save_count: 0,
            editor_seen: false,
        })
    }

    pub fn ram_kb(&self) -> u8 {
        (self.bus.ram.len() / 1024) as u8
    }
    pub fn set_wrx(&mut self, enabled: bool) {
        self.bus.wrx = enabled;
    }
    pub fn set_key(&mut self, key: Key, down: bool) {
        self.bus.keyboard.set(key, down);
    }
    pub fn release_keys(&mut self) {
        self.bus.keyboard.clear();
    }
    pub fn ticks(&self) -> u64 {
        self.clock.as_timestamp()
    }
    pub fn frames(&self) -> u64 {
        self.bus.ula.borrow().frames
    }
    pub fn video_fetches(&self) -> u64 {
        self.bus.ula.borrow().video_fetches
    }
    pub fn take_status(&mut self) -> Option<String> {
        self.status.take()
    }
    pub fn read_byte(&self, address: u16) -> u8 {
        self.bus.byte(address)
    }
    pub fn pixels(&self) -> Vec<u32> {
        self.bus.ula.borrow().pixels().to_vec()
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
        self.bus.ram.fill(0);
        self.release_keys();
        *self.bus.ula.borrow_mut() = Ula::default();
        self.next_frame = 0;
        self.status = Some("ZX81 reset".into());
        self.editor_seen = false;
    }

    pub fn run_frame(&mut self) {
        self.next_frame += video::FRAME_TSTATES;
        while self.ticks() < self.next_frame {
            self.step();
        }
    }

    pub fn run_frames(&mut self, count: usize) {
        for _ in 0..count {
            self.run_frame();
        }
    }

    pub fn boot(&mut self) -> io::Result<()> {
        for _ in 0..500 {
            self.run_frame();
            if self.editor_seen {
                self.run_frames(3);
                return Ok(());
            }
        }
        Err(io::Error::other("The ROM did not reach the BASIC editor"))
    }

    pub fn tap(&mut self, keys: &[Key]) {
        self.release_keys();
        for &key in keys {
            self.set_key(key, true);
        }
        self.run_frames(4);
        self.release_keys();
        self.run_frames(4);
    }

    /// Boot first, then load through the real BASIC command. Leave it ready for RUN.
    pub fn load_program(&mut self, program: Program) -> io::Result<()> {
        Program::from_bytes(program.bytes().to_vec(), self.ram_kb())?;
        self.reset();
        self.boot()?;
        self.mounted = Some(program);
        let before = self.load_count;
        self.tap(&[Key::J]); // LOAD
        self.tap(&[Key::Shift, Key::P]);
        self.tap(&[Key::Shift, Key::P]);
        self.tap(&[Key::Enter]);
        self.run_frames(20);
        if self.load_count == before {
            return Err(io::Error::other("BASIC did not finish loading the program"));
        }
        Ok(())
    }

    pub fn snapshot_program(&self) -> io::Result<Program> {
        let end = usize::from(self.bus.word(0x4014));
        if end <= START || end > 0x4000 + self.bus.ram.len() {
            return Err(program::invalid("Invalid E_LINE"));
        }
        Program::from_bytes(
            self.bus.ram[START - 0x4000..end - 0x4000].to_vec(),
            self.ram_kb(),
        )
    }

    fn step(&mut self) {
        if (0x04c1..=0x04d3).contains(&self.cpu.get_pc()) {
            self.editor_seen = true;
        }
        self.clock.halted = self.cpu.is_halt();
        let pending = self.bus.ula.borrow().nmi_pending;
        if pending && self.cpu.is_nmi_allowed() {
            self.bus.ula.borrow_mut().nmi_pending = false;
            self.clock.halted = false;
            self.cpu.nmi(&mut self.bus, &mut self.clock);
            return;
        }
        if !self.cpu.is_halt() && !self.cpu.is_after_prefix() {
            self.tape_trap();
        }
        if self.cpu.is_halt() {
            self.bus.ula.borrow_mut().irq = self.cpu.get_ir() & 0x40 == 0;
        }
        let _ = self
            .cpu
            .execute_next(&mut self.bus, &mut self.clock, None::<CpuDebugFn>);
    }

    fn tape_name(&self, mut address: u16) -> io::Result<String> {
        let mut name = String::new();
        for _ in 0..127 {
            let code = self.bus.byte(address);
            let ch = program::decode_character(code)
                .ok_or_else(|| program::invalid("The file name contains a graphics character"))?;
            name.push(ch);
            if code & 0x80 != 0 {
                return Ok(name);
            }
            address = address.wrapping_add(1);
        }
        Err(program::invalid("File name too long"))
    }

    fn tape_trap(&mut self) {
        let result = match self.cpu.get_pc() {
            // After the ROM's NAME routine has evaluated the string and stopped
            // SLOW mode. Its syntax-check path returns past these addresses.
            0x02fc => Some(self.save_from_basic()),
            0x0343 => Some(self.load_from_basic()),
            _ => None,
        };
        if let Some(Err(error)) = result {
            self.status = Some(format!("File error: {error}"));
            // The ROM's BREAK report unwinds its own calculator and machine stack.
            self.cpu.set_pc(0x03a6);
        }
    }

    fn save_from_basic(&mut self) -> io::Result<()> {
        let name = self.tape_name(self.cpu.get_reg16(StkReg16::HL))?;
        let path = program::file_path(&self.directory, &name)?;
        let program = self.snapshot_program()?;
        // A complete image replaces the old file atomically; failed writes keep it.
        let temp = self.directory.join(format!(
            ".zx81-rust-{}-{}.tmp",
            std::process::id(),
            self.save_count
        ));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        let result = (|| {
            file.write_all(program.bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, &path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result?;
        self.save_count += 1;
        self.status = Some(format!(
            "Saved: {} ({} bytes)",
            path.file_name().unwrap().to_string_lossy(),
            program.bytes().len()
        ));
        self.cpu.set_pc(0x0207); // normal SAVE/LOAD completion; restarts SLOW if needed
        Ok(())
    }

    fn load_from_basic(&mut self) -> io::Result<()> {
        let empty = self.cpu.get_flags().contains(CpuFlags::C);
        let program = if empty {
            self.mounted.clone().ok_or_else(|| {
                program::invalid("LOAD \"\" requires a previously opened program file")
            })?
        } else {
            let name = self.tape_name(self.cpu.get_reg16(StkReg16::DE))?;
            let path = self.resolve_load(&name)?;
            Program::read(&path, self.ram_kb())?
        };
        for (offset, &value) in program.bytes().iter().enumerate() {
            self.bus.put((START + offset) as u16, value);
        }
        let end = self.bus.word(0x4014);
        self.bus.put(end, 0x7f);
        self.bus.put(end + 1, 0x76);
        self.bus.put_word(0x401a, end + 2);
        self.bus.put_word(0x401c, end + 2);
        self.bus.put_word(0x4016, end + 1); // CH_ADD
        // Returning from a direct LOAD should leave the program in the editor.
        self.bus.put_word(0x4029, self.bus.word(0x400c)); // NXTLIN -> end of BASIC
        let flags = self.bus.byte(0x403b);
        self.bus.put(
            0x403b,
            (flags & !0x40) | if flags & 0x80 != 0 { 0 } else { 0x40 },
        );
        self.cpu.set_index16(Prefix::Yfd, 0x4000);
        self.cpu.set_pc(0x0207);
        self.load_count += 1;
        self.mounted = Some(program);
        self.status = Some("Program loaded. Type RUN (R) and Enter.".into());
        Ok(())
    }

    fn resolve_load(&self, name: &str) -> io::Result<PathBuf> {
        let path = program::file_path(&self.directory, name)?;
        if path.is_file() {
            return Ok(path);
        }
        let file_name = path.file_name().unwrap().to_string_lossy();
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&file_name)
            {
                return Ok(entry.path());
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("File {} not found", path.display()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Key::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "zx81-rust-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn command(m: &mut Machine, keys: &[&[Key]]) {
        for keys in keys {
            m.tap(keys);
        }
        m.tap(&[Enter]);
        // The genuine ROM reclaims and redraws its display after editing a line.
        m.editor_seen = false;
        m.boot().unwrap();
    }

    fn basic_line(m: &mut Machine) {
        command(
            m,
            &[&[Num1], &[Num0], &[P], &[Shift, P], &[H], &[I], &[Shift, P]],
        );
        assert_eq!(
            (0x407d..0x4087).map(|a| m.bus.byte(a)).collect::<Vec<_>>(),
            [0, 10, 6, 0, 0xf5, 0x0b, 0x2d, 0x2e, 0x0b, 0x76]
        );
    }

    fn program_bytes(m: &Machine) -> Vec<u8> {
        (0x407d..m.bus.word(0x400c))
            .map(|a| m.bus.byte(a))
            .collect()
    }

    #[test]
    fn rom_boots_and_every_ram_configuration_reaches_the_basic_editor() {
        for ram in [1, 2, 4, 8, 16] {
            let mut m = Machine::new(ram, std::env::temp_dir()).unwrap();
            m.boot().unwrap();
            assert!(m.frames() > 0);
            assert!(m.video_fetches() > 0);
            assert_eq!(m.bus.word(0x4004), 0x4000 + u16::from(ram) * 1024);
            assert!(
                m.pixels().contains(&0),
                "The K cursor must actually be drawn"
            );
            let before = m.pixels();
            m.tap(&[P]);
            assert!(before != m.pixels(), "Typing PRINT must change the raster");
            let end = m.bus.word(0x4014);
            assert_eq!(m.bus.byte(end), 0xf5);
            m.snapshot_program().unwrap();
        }
    }

    #[test]
    fn memory_mirrors_and_rom_protection_match_the_stock_machine() {
        let mut m = Machine::new(1, std::env::temp_dir()).unwrap();
        m.bus.put(0, 0xff);
        assert_eq!(m.bus.byte(0), ROM[0]);
        assert_eq!(m.bus.byte(0x2000), ROM[0]);
        m.bus.put(0x4000, 0x42);
        for address in [0x4000, 0x4400, 0x7c00, 0xc000, 0xfc00] {
            assert_eq!(m.bus.byte(address), 0x42);
        }
        assert!(Machine::new(3, std::env::temp_dir()).is_err());
    }

    #[test]
    fn m1_fetch_draws_rom_glyphs_inverse_characters_and_wrx_ram_pixels() {
        for (inverse, wrx) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut m = Machine::new(16, std::env::temp_dir()).unwrap();
            m.set_wrx(wrx);
            m.bus.put(0x4000, 0x2d | if inverse { 0x80 } else { 0 }); // H
            m.bus.put(0x4140, 0x55);
            m.bus
                .ula
                .borrow_mut()
                .advance(u64::from(video::LINE_TSTATES) * 16 + 100);
            let row = m.bus.ula.borrow().char_row;
            let ir = if wrx { 0x4140 } else { 0x1e40 };
            let expected = if wrx {
                0x55
            } else {
                ROM[0x1e00 + 0x2d * 8 + usize::from(row)]
            };
            let expected: u8 = expected ^ if inverse { 0xff } else { 0 };
            assert_eq!(m.bus.read_opcode(0xc000, ir, 0), 0); // character becomes NOP
            let mut ula = m.bus.ula.borrow_mut();
            ula.advance(4);
            ula.start_vsync();
            ula.advance(400);
            ula.output(0xff);
            assert_eq!(
                ula.pixels().iter().filter(|&&p| p == 0).count(),
                expected.count_ones() as usize
            );
        }
    }

    #[test]
    fn rom_fast_and_slow_commands_switch_display_mode_and_resume_typing() {
        let mut m = Machine::new(16, std::env::temp_dir()).unwrap();
        m.boot().unwrap();
        // Feed the ROM its native keyword token in an otherwise normally typed
        // direct command; all parsing and mode switching is executed by the Z80.
        for (token, slow) in [(0xe5, false), (0xe4, true)] {
            m.tap(&[P]);
            let edit = m.bus.word(0x4014);
            m.bus.put(edit, token);
            command(&mut m, &[]);
            assert_eq!(m.bus.byte(0x403b) & 0x80 != 0, slow);
            m.tap(&[P]);
            assert_eq!(m.bus.byte(m.bus.word(0x4014)), 0xf5);
            m.tap(&[Shift, Num0]);
        }
    }

    #[test]
    fn basic_save_named_load_startup_load_and_run_round_trip() {
        let temp = Temp::new();
        let mut m = Machine::new(16, &temp.0).unwrap();
        m.boot().unwrap();
        basic_line(&mut m);
        let expected = program_bytes(&m);
        command(
            &mut m,
            &[&[S], &[Shift, P], &[T], &[E], &[S], &[T], &[Shift, P]],
        );
        assert_eq!(m.save_count, 1);
        let saved = Program::read(&temp.0.join("TEST.p"), 16).unwrap();
        assert_eq!(&saved.bytes()[0x407d - START..0x4087 - START], &expected);

        // A named LOAD must really restore a different program, not merely leave
        // the existing RAM alone. Also exercise case-insensitive host lookup.
        fs::rename(temp.0.join("TEST.p"), temp.0.join("test.p")).unwrap();
        command(&mut m, &[&[A]]); // NEW
        assert!(program_bytes(&m).is_empty());
        command(
            &mut m,
            &[&[J], &[Shift, P], &[T], &[E], &[S], &[T], &[Shift, P]],
        );
        assert_eq!(program_bytes(&m), expected);
        assert_eq!(m.load_count, 1);
        assert!(m.pixels().contains(&0));

        let mut fresh = Machine::new(16, &temp.0).unwrap();
        fresh.load_program(saved).unwrap();
        assert_eq!(program_bytes(&fresh), expected);
        assert_eq!(fresh.bus.byte(fresh.bus.word(0x400c) + 1), 0); // loaded, not RUN
        command(&mut fresh, &[&[R]]);
        let display = fresh.bus.word(0x400c);
        assert_eq!(fresh.bus.byte(display + 1), 0x2d); // H printed by the actual BASIC
        assert_eq!(fresh.bus.byte(display + 2), 0x2e); // I
        assert_eq!(fresh.bus.byte(0x4000), 0xff); // normal completion

        // Saving again replaces a complete image and leaves no temporary files.
        command(
            &mut fresh,
            &[&[S], &[Shift, P], &[T], &[E], &[S], &[T], &[Shift, P]],
        );
        Program::read(&temp.0.join("TEST.p"), 16).unwrap();
        command(
            &mut fresh,
            &[&[S], &[Shift, P], &[T], &[E], &[S], &[T], &[Shift, P]],
        );
        assert_eq!(fresh.save_count, 2);
        assert!(
            fs::read_dir(&temp.0).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp"))
        );
    }

    #[test]
    fn basic_file_errors_return_to_editor_and_preserve_program() {
        let temp = Temp::new();
        let mut m = Machine::new(16, &temp.0).unwrap();
        m.boot().unwrap();
        basic_line(&mut m);
        let before = program_bytes(&m);
        command(&mut m, &[&[J], &[Shift, P], &[X], &[Shift, P]]);
        assert_eq!(m.load_count, 0);
        assert!(m.take_status().unwrap().contains("not found"));
        assert_eq!(program_bytes(&m), before);
        fs::create_dir(temp.0.join("X.p")).unwrap();
        command(&mut m, &[&[S], &[Shift, P], &[X], &[Shift, P]]);
        assert_eq!(m.save_count, 0);
        assert!(m.take_status().unwrap().starts_with("File error"));
        assert!(temp.0.join("X.p").is_dir());
        assert_eq!(program_bytes(&m), before);
        command(&mut m, &[&[R]]);
        assert_eq!(m.bus.byte(m.bus.word(0x400c) + 1), 0x2d);
    }

    #[test]
    fn reset_releases_input_and_returns_to_a_fresh_basic() {
        let mut m = Machine::new(16, std::env::temp_dir()).unwrap();
        m.boot().unwrap();
        m.set_key(Shift, true);
        m.set_key(P, true);
        m.reset();
        m.boot().unwrap();
        assert_eq!(m.bus.keyboard.read(0), 0xff);
        assert!(program_bytes(&m).is_empty());
        assert!(m.pixels().contains(&0));
    }
}
