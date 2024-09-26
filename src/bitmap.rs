#![allow(dead_code)]

use bit_vec::BitVec;

pub struct Bitmap {
    pub w: usize,
    pub h: usize,
    pub data: BitVec,
}
impl Bitmap {
    pub fn new(w: usize, h: usize, on: bool) -> Self {
        let data = BitVec::from_elem(w * h, on);
        Bitmap { w, h, data }
    }

    /// Crop Bitmap to a new size. Out of bounds positions and sizes will panic.
    pub fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> Self {
        assert!(x <= self.w && y <= self.h);
        assert!(w <= self.w - x && h <= self.h - y);
        let mut data = BitVec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                data.push(self.data[x + y * self.w]);
            }
        }
        Self { w, h, data }
    }

    /// Blit another Bitmap onto this one. Bounds will *not* be expanded.
    pub fn blit(&mut self, other: &Bitmap, x: isize, y: isize) {
        for sy in 0..self.h {
            for sx in 0..self.w {
                let ox = sx as isize - x;
                let oy = sy as isize - y;
                if ox >= 0 && ox < other.w as isize && oy >= 0 && oy < other.h as isize {
                    let si = sx + sy * self.w;
                    let oi = ox as usize + oy as usize * other.w;
                    self.data.set(si, other.data[oi]);
                }
            }
        }
    }

    /// Inverts all pixels in the bitmap.
    pub fn invert(&mut self) {
        self.data.negate();
    }
}
