use std::{
    io::Read,
    ops::Div,
    time::{Duration, SystemTime},
};

use clap::Parser;
use image::io::Reader as ImageReader;

const SCREEN_WIDTH: u8 = 128;
const SCREEN_HEIGHT: u8 = 64;
const REPORT_SPLIT_SZ: u8 = 64;
const REPORT_SIZE: usize = 1024;
type DrawReport = [u8; REPORT_SIZE];

struct Bitmap {
    w: u8,
    h: u8,
    pixels: Vec<bool>,
}
struct Drawable {
    x: u8,
    y: u8,
    bitmap: Bitmap,
}
impl Bitmap {
    fn from_image(img: &image::DynamicImage, threshold: usize) -> Bitmap {
        Bitmap {
            w: img.width() as u8,
            h: img.height() as u8,
            pixels: img
                .to_rgb8()
                .pixels()
                .map(|p| ((p.0[0] as usize) + (p.0[1] as usize) + (p.0[2] as usize)) / 3 >= threshold)
                .collect::<Vec<bool>>(),
        }
    }
    fn crop(&self, x: u8, y: u8, w: u8, h: u8) -> Bitmap {
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
    fn from_bitmap(bitmap: Bitmap, x: u8, y: u8) -> Drawable {
        Drawable { x, y, bitmap }
    }
    fn rect(x: u8, y: u8, w: u8, h: u8, on: bool) -> Drawable {
        Drawable {
            x,
            y,
            bitmap: Bitmap {
                w,
                h,
                pixels: (0..(w as usize) * (h as usize)).map(|_| on).collect::<Vec<bool>>(),
            },
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
        report[2] = self.x;
        report[3] = self.y;
        report[4] = self.bitmap.w;
        report[5] = self.bitmap.h;
        // TODO: this calculation might be incorrect. do proper testing for corruption by mixing weird x/y/w/h combinations.
        let stride_h = (self.bitmap.h + self.y % 8).div_ceil(8) * 8;
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
struct ImageArgs {
    #[arg(short = 'x', long, help = "Screen X offset for draw commands", default_value = "0")]
    screen_x: u8,

    #[arg(short = 'y', long, help = "Screen Y offset for draw commands", default_value = "0")]
    screen_y: u8,

    #[arg(
        short = 'T',
        long,
        help = "Grayscale threshold for converting images to 1-bit",
        default_value = "100"
    )]
    threshold: usize,

    #[arg(
        short = 'C',
        long,
        help = "Clear the entire screen to black before drawing",
        default_value = "false"
    )]
    clear: bool,
}

#[derive(Parser)]
#[command(about = "SteelSeries Arctis Nova Pro Wireless OLED drawing utility")]
enum Args {
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

        #[arg(short = 'r', long, help = "Amount of frames to show per second", default_value = "1")]
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
    #[command(about = "Clear the entire screen to black")]
    Clear,
    #[command(about = "Fill the entire screen to white")]
    Fill,
}

fn draw(dev: &hidapi::HidDevice, drawable: &Drawable) {
    for d in drawable.crop_to_screen().split_for_reports() {
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
        Args::Img { path, image_args } => {
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
            let drawable = Drawable::from_bitmap(bitmap, image_args.screen_x, image_args.screen_y).crop_to_screen();
            if image_args.clear {
                draw(&dev, &Drawable::rect(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT, false));
            }
            draw(&dev, &drawable)
        }
        Args::Anim {
            framerate,
            loops,
            paths,
            image_args,
        } => {
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
                    Drawable::from_bitmap(bitmap, image_args.screen_x, image_args.screen_y).crop_to_screen()
                })
                .collect();
            if image_args.clear {
                draw(&dev, &Drawable::rect(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT, false));
            }
            let draw_animation = || {
                let period = Duration::from_secs(1).div(framerate);
                let mut next_frame = SystemTime::now() + period;
                draw(&dev, &drawables[0]);
                for drawable in drawables.iter().skip(1) {
                    let time = SystemTime::now();
                    if time < next_frame {
                        std::thread::sleep(next_frame.duration_since(time).unwrap());
                    } else {
                        println!("fell behind - framerate too fast");
                    }
                    draw(&dev, drawable);
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
        Args::Clear => draw(&dev, &Drawable::rect(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT, false)),
        Args::Fill => draw(&dev, &Drawable::rect(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT, true)),
    }
}
