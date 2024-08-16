use clap::Parser;
use core::str;
use image::io::Reader as ImageReader;
use rusttype::{point, Font, Scale};
use std::{
    io::Read,
    ops::Div,
    time::{Duration, SystemTime},
};

const SCREEN_WIDTH: usize = 128;
const SCREEN_HEIGHT: usize = 64;
const REPORT_SPLIT_SZ: usize = 64;
const REPORT_SIZE: usize = 1024;
type DrawReport = [u8; REPORT_SIZE];

struct Bitmap {
    w: usize,
    h: usize,
    pixels: Vec<bool>,
}
struct Drawable {
    x: usize,
    y: usize,
    bitmap: Bitmap,
}
impl Bitmap {
    fn new(w: usize, h: usize, on: bool) -> Bitmap {
        Bitmap {
            w,
            h,
            pixels: (0..(w as usize) * (h as usize)).map(|_| on).collect::<Vec<bool>>(),
        }
    }
    fn get(&self, x: usize, y: usize) -> bool {
        self.pixels[y * self.w + x]
    }
    fn set(&mut self, x: usize, y: usize, on: bool) {
        self.pixels[y * self.w + x] = on;
    }
    fn from_image(img: &image::DynamicImage, threshold: usize) -> Bitmap {
        Bitmap {
            w: img.width() as usize,
            h: img.height() as usize,
            pixels: img
                .to_rgb8()
                .pixels()
                .map(|p| ((p.0[0] as usize) + (p.0[1] as usize) + (p.0[2] as usize)) / 3 >= threshold)
                .collect::<Vec<bool>>(),
        }
    }
    fn from_text(text: &str) -> Bitmap {
        // heavily based on https://github.com/redox-os/rusttype/blob/c1e820b4418c0bfad9bf8753acbb90e872408a6e/dev/examples/image.rs#L4
        // TODO: line breaks
        let font = Font::try_from_bytes(include_bytes!("../fonts/PixelOperator.ttf")).unwrap();
        let scale = Scale::uniform(16.0);
        let glyphs: Vec<_> = font.layout(&text, scale, point(0.0, 0.0)).collect();
        let w_offset = glyphs
            .iter()
            .map(|g| -g.pixel_bounding_box().map(|bb| bb.min.x).unwrap_or(0))
            .max()
            .unwrap_or(0);
        let h_offset = glyphs
            .iter()
            .map(|g| -g.pixel_bounding_box().map(|bb| bb.min.y).unwrap_or(0))
            .max()
            .unwrap_or(0);
        let w = glyphs
            .iter()
            .map(|g| g.pixel_bounding_box().map(|bb| bb.max.x + 1).unwrap_or(0) + w_offset)
            .max()
            .unwrap_or(0) as usize;
        let h = glyphs
            .iter()
            .map(|g| g.pixel_bounding_box().map(|bb| bb.max.y + 1).unwrap_or(0) + h_offset)
            .max()
            .unwrap_or(0) as usize;
        //let v_metrics = font.v_metrics(scale);
        //let h = (v_metrics.ascent - v_metrics.descent).ceil() as usize;
        let mut pixels = vec![false; w * h];
        for glyph in glyphs {
            if let Some(bb) = glyph.pixel_bounding_box() {
                glyph.draw(|x, y, v| {
                    let px = (x as i32 + bb.min.x) as usize;
                    let py = (y as i32 + h_offset + bb.min.y) as usize;
                    pixels[py * w + px] = v > 0.5;
                })
            }
        }
        Bitmap { w, h, pixels }
    }
    fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> Bitmap {
        let mut pixels = Vec::<bool>::with_capacity((w as usize) * (h as usize));
        for ny in 0..h {
            for nx in 0..w {
                pixels.push(
                    self.pixels[((ny as usize) + (y as usize)) * (self.w as usize) + ((nx as usize) + (x as usize))],
                );
            }
        }
        Bitmap { w, h, pixels }
    }
}
impl Drawable {
    fn from_bitmap(bitmap: Bitmap, x: usize, y: usize) -> Drawable {
        Drawable { x, y, bitmap }
    }
    fn rect(x: usize, y: usize, w: usize, h: usize, on: bool) -> Drawable {
        Drawable {
            x,
            y,
            bitmap: Bitmap::new(w, h, on),
        }
    }
    fn crop_to_screen(&self) -> Drawable {
        let x = std::cmp::min(SCREEN_WIDTH - 1, self.x);
        let y = std::cmp::min(SCREEN_HEIGHT - 1, self.y);
        let w = std::cmp::min(SCREEN_WIDTH - x, self.bitmap.w);
        let h = std::cmp::min(SCREEN_HEIGHT - y, self.bitmap.h);
        Drawable {
            x,
            y,
            bitmap: self.bitmap.crop(0, 0, w, h),
        }
    }
    fn blit(&mut self, other: &Drawable) {
        for bx in 0..other.bitmap.w {
            for by in 0..other.bitmap.h {
                self.bitmap.set(other.x + bx, other.y + by, other.bitmap.get(bx, by));
            }
        }
    }
    fn with_clear(&self) -> Drawable {
        let mut screen = Drawable::rect(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT, false);
        screen.blit(self);
        screen
    }
    fn as_hid_report(&self) -> DrawReport {
        let mut report: DrawReport = [0; REPORT_SIZE];
        // TODO: figure out the actual limits for a single report
        if !((self.bitmap.w <= REPORT_SPLIT_SZ && self.bitmap.h <= REPORT_SPLIT_SZ)
            || ((self.bitmap.w as usize) * (self.bitmap.h as usize) <= 1024))
        {
            panic!("bitmap too large for one report");
        } else if self.bitmap.pixels.len() < (self.bitmap.w as usize) * (self.bitmap.h as usize) {
            panic!("pixels.len smaller than w*h");
        }
        report[0] = 0x06; // hid report id
        report[1] = 0x93; // steelseries command id? unknown
        report[2] = self.x as u8;
        report[3] = self.y as u8;
        report[4] = self.bitmap.w as u8;
        report[5] = self.bitmap.h as u8;
        // NOTE: this stride calculation *seems* to work, but maybe i'm missing something - if you get corrupt stuff on the screen varying on position, this is why
        let stride_h = self.bitmap.h.div_ceil(8) * 8;
        for y in 0..self.bitmap.h {
            for x in 0..self.bitmap.w {
                // NOTE: report has columns rather than rows
                let ri = (x as usize) * (stride_h as usize) + (y as usize);
                let pi = (y as usize) * (self.bitmap.w as usize) + (x as usize);
                report[(ri / 8) + 6] |= (self.bitmap.pixels[pi] as u8) << (ri % 8);
            }
        }
        report
    }
    fn split_for_reports(&self) -> Vec<Drawable> {
        let mut vec = Vec::<Drawable>::new();
        let splits = self.bitmap.w.div_ceil(REPORT_SPLIT_SZ);
        for i in 0..splits {
            vec.push(Drawable {
                x: self.x + i * REPORT_SPLIT_SZ,
                y: self.y,
                bitmap: self.bitmap.crop(
                    i * REPORT_SPLIT_SZ,
                    0,
                    std::cmp::min(REPORT_SPLIT_SZ, self.bitmap.w - i * REPORT_SPLIT_SZ),
                    self.bitmap.h,
                ),
            });
        }
        vec
    }
}

#[derive(clap::Args)]
struct DrawArgs {
    #[arg(short = 'x', long, help = "Screen X offset for draw commands", default_value = "0")]
    screen_x: usize,

    #[arg(short = 'y', long, help = "Screen Y offset for draw commands", default_value = "0")]
    screen_y: usize,

    #[arg(
        short = 'C',
        long,
        help = "Clear the entire screen to black before drawing",
        default_value = "false"
    )]
    clear: bool,
    //
    // TODO: invert
}

#[derive(clap::Args)]
struct ImageArgs {
    #[command(flatten)]
    draw_args: DrawArgs,

    #[arg(
        short = 'T',
        long,
        help = "Grayscale threshold for converting images to 1-bit",
        default_value = "100"
    )]
    threshold: usize,
}

#[derive(Parser)]
#[command(about = "SteelSeries Arctis Nova Pro Wireless OLED drawing utility")]
enum Args {
    #[command(about = "Clear the entire screen to black")]
    Clear,

    #[command(about = "Fill the entire screen to white")]
    Fill,

    #[command(about = "Draw some text")]
    Text {
        #[command(flatten)]
        draw_args: DrawArgs,

        #[arg(help = "Text, or omitted for stdin", index = 1)]
        text: Option<String>,
        //
        // TODO: custom font
        // TODO: font size
        // TODO: alignment (left/center/right)
        // TODO: some way to update text from stdin without re-invoking the command
    },

    #[command(about = "Draw an image")]
    Img {
        #[command(flatten)]
        image_args: ImageArgs,

        #[arg(help = "Image path, or - for stdin", index = 1)]
        path: String,
    },

    #[command(about = "Draw a sequence of images")]
    Anim {
        #[command(flatten)]
        image_args: ImageArgs,

        #[arg(short = 'r', long, help = "Frames to show per second (fps)", default_value = "1")]
        framerate: u32,

        #[arg(
            short = 'l',
            long,
            help = "Amount of repetitions, or 0 for infinite",
            default_value = "1"
        )]
        loops: usize,

        #[arg(help = "Image paths", index = 1)]
        paths: Vec<String>,
    },
}

fn draw(dev: &hidapi::HidDevice, drawable: &Drawable, clear: bool) {
    let drawable = drawable.crop_to_screen();
    let drawable = if clear { drawable.with_clear() } else { drawable };
    for d in drawable.split_for_reports() {
        dev.send_feature_report(&d.as_hid_report()).unwrap();
    }
}

fn main() {
    let args = Args::parse();

    let api = hidapi::HidApi::new().unwrap();
    let dev = api
        .device_list()
        .find(|d| d.vendor_id() == 0x1038 && d.product_id() == 0x12e0 && d.interface_number() == 4)
        .expect("Device not found")
        .open_device(&api)
        .expect("Failed to open device");

    match args {
        Args::Clear => draw(&dev, &Drawable::rect(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT, false), false),
        Args::Fill => draw(&dev, &Drawable::rect(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT, true), false),
        Args::Text { text, draw_args } => {
            let text = text.unwrap_or_else(|| {
                let mut buf = Vec::<u8>::new();
                std::io::stdin()
                    .read_to_end(&mut buf)
                    .expect("Failed to read from stdin");
                String::from_utf8(buf).unwrap()
            });
            let bitmap = Bitmap::from_text(&text);
            let drawable = Drawable::from_bitmap(bitmap, draw_args.screen_x, draw_args.screen_y).crop_to_screen();
            draw(&dev, &drawable, draw_args.clear);
        }
        Args::Img { path, image_args } => {
            let draw_args = &image_args.draw_args;
            let img = if path == "-" {
                let mut buf = Vec::<u8>::new();
                std::io::stdin()
                    .read_to_end(&mut buf)
                    .expect("Failed to read from stdin");
                image::load_from_memory(&buf).expect("Failed to load image from stdin")
            } else {
                ImageReader::open(path)
                    .expect("Failed to open image")
                    .decode()
                    .expect("Failed to decode image")
            };
            let bitmap = Bitmap::from_image(&img, image_args.threshold);
            let drawable = Drawable::from_bitmap(bitmap, draw_args.screen_x, draw_args.screen_y).crop_to_screen();
            draw(&dev, &drawable, false);
        }
        Args::Anim {
            framerate,
            loops,
            paths,
            image_args,
        } => {
            let draw_args = &image_args.draw_args;
            if framerate == 0 {
                panic!("Framerate must be non-zero");
            } else if paths.is_empty() {
                panic!("No image paths");
            }
            let drawables: Vec<Drawable> = paths
                .iter()
                .map(|path| {
                    let img = ImageReader::open(path)
                        .expect("Failed to open image")
                        .decode()
                        .expect("Failed to decode image");
                    let bitmap = Bitmap::from_image(&img, image_args.threshold);
                    Drawable::from_bitmap(bitmap, draw_args.screen_x, draw_args.screen_y).crop_to_screen()
                })
                .collect();
            let draw_animation = || {
                let period = Duration::from_secs(1).div(framerate);
                let mut next_frame = SystemTime::now() + period;
                draw(&dev, &drawables[0], draw_args.clear);
                for drawable in drawables.iter().skip(1) {
                    let time = SystemTime::now();
                    if time < next_frame {
                        std::thread::sleep(next_frame.duration_since(time).unwrap());
                    } else {
                        println!("fell behind - framerate too fast");
                    }
                    draw(&dev, drawable, false);
                    next_frame += period;
                }
            };
            if loops == 0 {
                loop {
                    draw_animation();
                }
            } else {
                for _ in 0..loops {
                    draw_animation();
                }
            }
        }
    }
}
