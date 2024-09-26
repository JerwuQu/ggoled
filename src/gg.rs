use crate::bitmap::Bitmap;
use anyhow::bail;
use hidapi::{HidApi, HidDevice};
use std::cmp::min;

// NOTE: these work for Arctis Nova Pro but might not for different products!
const REPORT_SPLIT_SZ: usize = 64;
const REPORT_SIZE: usize = 1024;

type DrawReport = [u8; REPORT_SIZE];

struct ReportDrawable<'a> {
    bitmap: &'a Bitmap,
    w: usize,
    h: usize,
    dst_x: usize,
    dst_y: usize,
    src_x: usize,
    src_y: usize,
}

pub struct Device {
    dev: HidDevice,
    pub width: usize,
    pub height: usize,
}
impl Device {
    /// Connect to a SteelSeries GG device.
    pub fn connect() -> anyhow::Result<Device> {
        let api = HidApi::new().unwrap();
        let Some(dev_info) = api.device_list().find(|d| {
            d.vendor_id() == 0x1038 // SteelSeries
        && [
            0x12cb, // Arctis Nova Pro Wired
            0x12e0, // Arctis Nova Pro Wireless
        ].contains(&d.product_id()) && d.interface_number() == 4
        }) else {
            bail!("Device not found");
        };
        let Ok(dev) = dev_info.open_device(&api) else {
            bail!("Failed to open device");
        };

        Ok(Device {
            dev,
            width: 128,
            height: 64,
        })
    }

    // Creates a HID report for a `ReportDrawable`
    // The Bitmap must already be within the report limits (from `split_for_report`)
    fn create_report(&self, d: &ReportDrawable) -> DrawReport {
        let mut report: DrawReport = [0; REPORT_SIZE];
        report[0] = 0x06; // hid report id
        report[1] = 0x93; // steelseries command id? unknown
        report[2] = d.dst_x as u8;
        report[3] = d.dst_y as u8;
        report[4] = d.w as u8;
        report[5] = d.h as u8;
        let stride_h = (d.dst_y.wrapping_rem(8) + d.h).div_ceil(8) * 8; // TODO: fuzz this with all x/y/w/h combinations
        for y in 0..d.h {
            for x in 0..d.w {
                // NOTE: report has columns rather than rows
                let ri = x * stride_h + y;
                let pi = (d.src_y + y) * d.bitmap.w + (d.src_x + x);
                report[(ri / 8) + 6] |= (d.bitmap.data[pi] as u8) << (ri % 8);
            }
        }
        report
    }

    // Splits up a `Bitmap` to be appropriately sized for being able to send over USB HID
    fn prepare_for_report<'a>(&self, bitmap: &'a Bitmap, x: isize, y: isize) -> Vec<ReportDrawable<'a>> {
        let mut w = bitmap.w;
        let mut h = bitmap.h;

        // Handle negative x/y by moving src_x/src_y
        let (x, src_x) = if x < 0 {
            w -= (-x) as usize;
            (0, (-x) as usize)
        } else {
            (x as usize, 0)
        };
        let (y, src_y) = if y < 0 {
            h -= (-y) as usize;
            (0, (-y) as usize)
        } else {
            (y as usize, 0)
        };

        // Crop size to screen
        let x = min(x, self.width);
        let y = min(y, self.height);
        if x + w >= self.width {
            w = self.width - x;
        }
        if y + h >= self.height {
            h = self.height - y;
        }

        // Split
        let mut vec = Vec::<ReportDrawable<'a>>::new();
        let splits = w.div_ceil(REPORT_SPLIT_SZ);
        for i in 0..splits {
            vec.push(ReportDrawable {
                bitmap,
                w: min(REPORT_SPLIT_SZ, w - i * REPORT_SPLIT_SZ),
                h,
                dst_x: x + (i * REPORT_SPLIT_SZ),
                dst_y: y,
                src_x: src_x + i * REPORT_SPLIT_SZ,
                src_y,
            });
        }
        vec
    }

    pub fn draw(&self, bitmap: &Bitmap, x: isize, y: isize) -> anyhow::Result<()> {
        let drawables = self.prepare_for_report(bitmap, x, y);
        for drawable in drawables {
            let report = self.create_report(&drawable);
            self.dev.send_feature_report(&report)?;
        }
        Ok(())
    }
}
