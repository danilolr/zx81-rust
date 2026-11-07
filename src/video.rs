//! ZX81 ULA timing and monochrome shift register, driven by Z80 T-states.
use std::{
    fs::File,
    io::{self, Write},
    path::Path,
};

pub const LINE_TSTATES: u16 = 207;
pub const FRAME_TSTATES: u64 = 65_000;
pub const WIDTH: usize = 384;
pub const HEIGHT: usize = 288;
const WHITE: u32 = 0xffffff;

pub(crate) struct Ula {
    pub ticks: u64,
    pub line_t: u16,
    pub char_row: u8,
    pub nmi_enabled: bool,
    pub nmi_pending: bool,
    pub irq: bool,
    pub frames: u64,
    pub video_fetches: u64,
    row: usize,
    shift: u8,
    bits: u8,
    vsync_since: Option<u64>,
    last_frame: u64,
    blanked: bool,
    drawing: Vec<u32>,
    displayed: Vec<u32>,
}

impl Default for Ula {
    fn default() -> Self {
        Self {
            ticks: 0,
            line_t: 0,
            char_row: 0,
            nmi_enabled: false,
            nmi_pending: false,
            irq: false,
            frames: 0,
            video_fetches: 0,
            row: 0,
            shift: 0,
            bits: 0,
            vsync_since: None,
            last_frame: 0,
            blanked: false,
            drawing: vec![WHITE; WIDTH * HEIGHT],
            displayed: vec![WHITE; WIDTH * HEIGHT],
        }
    }
}

impl Ula {
    pub fn advance(&mut self, count: u64) {
        for _ in 0..count {
            if self.vsync_since.is_none() {
                if self.line_t == 16 {
                    self.char_row = (self.char_row + 1) & 7;
                    self.nmi_pending |= self.nmi_enabled;
                }
                if self.line_t == 32 {
                    self.row = self.row.saturating_add(1);
                }
                let x = if self.line_t >= 32 {
                    self.line_t - 32
                } else {
                    self.line_t + 175
                } as usize
                    * 2;
                for dot in 0..2 {
                    let black = self.bits > 0 && self.shift & 0x80 != 0;
                    if self.row >= 8
                        && self.row < HEIGHT + 8
                        && (22..WIDTH + 22).contains(&(x + dot))
                    {
                        self.drawing[(self.row - 8) * WIDTH + x + dot - 22] =
                            if black { 0 } else { WHITE };
                    }
                    self.shift <<= 1;
                    self.bits = self.bits.saturating_sub(1);
                }
            }
            self.line_t = (self.line_t + 1) % LINE_TSTATES;
            self.ticks += 1;
        }
        // FAST mode deliberately stops the display. Do not leave a stale image.
        if !self.blanked
            && self.vsync_since.is_some()
            && self.ticks - self.last_frame > FRAME_TSTATES * 2
        {
            self.displayed.fill(WHITE);
            self.blanked = true;
        }
    }

    pub fn shift(&mut self, pixels: u8) {
        self.shift = pixels;
        self.bits = 8;
        self.video_fetches += 1;
    }

    pub fn hsync(&self) -> bool {
        (16..32).contains(&self.line_t) && self.vsync_since.is_none()
    }

    pub fn start_vsync(&mut self) {
        if !self.nmi_enabled {
            self.vsync_since.get_or_insert(self.ticks);
        }
    }

    pub fn output(&mut self, port: u16) {
        if let Some(start) = self.vsync_since.take() {
            if self.ticks - start >= 300 {
                std::mem::swap(&mut self.drawing, &mut self.displayed);
                self.drawing.fill(WHITE);
                self.row = 0;
                self.frames += 1;
                self.last_frame = self.ticks;
                self.blanked = false;
            }
            self.char_row = 0;
        }
        // A0 low enables the NMI generator; A1 low disables it. FE/FD are
        // the ports used by Sinclair's ROM. Other OUTs still terminate VSYNC.
        if port & 1 == 0 {
            self.nmi_enabled = true;
        } else if port & 2 == 0 {
            self.nmi_enabled = false;
            self.nmi_pending = false;
        }
    }

    pub fn pixels(&self) -> &[u32] {
        &self.displayed
    }
}

pub fn write_ppm(path: &Path, pixels: &[u32]) -> io::Result<()> {
    if pixels.len() != WIDTH * HEIGHT {
        return Err(io::Error::other("Invalid image dimensions"));
    }
    let mut file = io::BufWriter::new(File::create(path)?);
    write!(file, "P6\n{WIDTH} {HEIGHT}\n255\n")?;
    for pixel in pixels {
        file.write_all(&[(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8])?;
    }
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hsync_counts_glyph_rows_and_latches_a_border_nmi() {
        let mut ula = Ula {
            nmi_enabled: true,
            ..Ula::default()
        };
        ula.advance(16);
        assert!(!ula.nmi_pending);
        ula.advance(1);
        assert!(ula.nmi_pending);
        assert_eq!(ula.char_row, 1);
        ula.nmi_pending = false;
        ula.advance(u64::from(LINE_TSTATES));
        assert!(ula.nmi_pending);
        assert_eq!(ula.char_row, 2);
        ula.output(0xfd);
        assert!(!ula.nmi_enabled);
        assert!(!ula.nmi_pending);
    }

    #[test]
    fn vsync_closes_a_frame_and_fast_mode_blanks_stale_video() {
        let mut ula = Ula::default();
        ula.drawing[100] = 0;
        ula.start_vsync();
        ula.advance(400);
        ula.output(0xff);
        assert_eq!(ula.frames, 1);
        assert_eq!(ula.pixels()[100], 0);
        assert_eq!(ula.char_row, 0);
        ula.start_vsync();
        ula.advance(FRAME_TSTATES * 3);
        assert!(ula.pixels().iter().all(|&p| p == WHITE));
    }
}
