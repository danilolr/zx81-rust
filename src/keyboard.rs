//! The eight active-low, five-key half rows selected by address lines A8–A15.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Key {
    Shift,
    Z,
    X,
    C,
    V,
    A = 8,
    S,
    D,
    F,
    G,
    Q = 16,
    W,
    E,
    R,
    T,
    Num1 = 24,
    Num2,
    Num3,
    Num4,
    Num5,
    Num0 = 32,
    Num9,
    Num8,
    Num7,
    Num6,
    P = 40,
    O,
    I,
    U,
    Y,
    Enter = 48,
    L,
    K,
    J,
    H,
    Space = 56,
    Period,
    M,
    N,
    B,
}

#[derive(Clone, Default)]
pub struct Keyboard {
    rows: [u8; 8],
}

impl Keyboard {
    pub fn set(&mut self, key: Key, down: bool) {
        let id = key as usize;
        let mask = 1 << (id % 8);
        if down {
            self.rows[id / 8] |= mask;
        } else {
            self.rows[id / 8] &= !mask;
        }
    }

    pub fn clear(&mut self) {
        self.rows = [0; 8];
    }

    pub fn read(&self, port: u16) -> u8 {
        let mut result = 0xff;
        for (row, pressed) in self.rows.iter().enumerate() {
            if port & (0x100 << row) == 0 {
                result &= !pressed;
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn half_rows_are_active_low_and_can_be_combined() {
        let mut keys = Keyboard::default();
        keys.set(Key::Shift, true);
        keys.set(Key::A, true);
        keys.set(Key::G, true);
        assert_eq!(keys.read(0xfefe), 0xfe);
        assert_eq!(keys.read(0xfdfe), 0xee);
        assert_eq!(keys.read(0xfcfe), 0xee);
        assert_eq!(keys.read(0xfffe), 0xff);
        keys.clear();
        assert_eq!(keys.read(0x00fe), 0xff);
    }
}
