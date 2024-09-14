use clap::{Parser, ValueEnum};
use core::str;
use image::{codecs::gif::GifDecoder, io::Reader as ImageReader, AnimationDecoder, ImageFormat};
use rusttype::{point, Font, Scale};
use std::{
    cmp::{max, min},
    io::{stdin, Read},
    ops::Div,
    str::FromStr,
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
    x: isize,
    y: isize,
    bitmap: Bitmap,
}
impl Bitmap {
    fn new(w: usize, h: usize, on: bool) -> Bitmap {
        Bitmap {
            w,
            h,
            pixels: (0..w * h).map(|_| on).collect::<Vec<bool>>(),
        }
    }
    fn get(&self, x: isize, y: isize) -> bool {
        x >= 0 && y >= 0 && x < self.w as isize && y < self.h as isize && self.pixels[y as usize * self.w + x as usize]
    }
    fn set(&mut self, x: isize, y: isize, on: bool) {
        if x >= 0 && y >= 0 && x < self.w as isize && y < self.h as isize {
            self.pixels[y as usize * self.w + x as usize] = on;
        }
    }
    fn from_image(img: &image::RgbaImage, threshold: usize) -> Bitmap {
        Bitmap {
            w: img.width() as usize,
            h: img.height() as usize,
            pixels: img
                .pixels()
                .map(|p| ((p.0[0] as usize) + (p.0[1] as usize) + (p.0[2] as usize)) / 3 >= threshold)
                .collect::<Vec<bool>>(),
        }
    }
    fn from_dynimage(img: &image::DynamicImage, threshold: usize) -> Bitmap {
        Self::from_image(&img.to_rgba8(), threshold)
    }
    // initially based on https://github.com/redox-os/rusttype/blob/c1e820b4418c0bfad9bf8753acbb90e872408a6e/dev/examples/image.rs#L4
    fn from_text(text: &str, alignment: Alignment) -> Bitmap {
        let clean_text = text.replace('\r', "");
        let text_lines = clean_text.split('\n');

        let font = Font::try_from_bytes(include_bytes!("../fonts/PixelOperator.ttf")).unwrap();

        let scale = Scale::uniform(16.0);
        let v_metrics = font.v_metrics(scale);
        let line_h = (v_metrics.ascent - v_metrics.descent).ceil();

        // collect all glyphs for each line of text and calculate bounds
        let mut line_glyphs = vec![];
        let mut block_w = 0;
        let mut block_h_offset = 0;
        let mut block_h = 0;
        for (yi, text_line) in text_lines.enumerate() {
            let glyphs: Vec<_> = font.layout(text_line, scale, point(0.0, yi as f32 * line_h)).collect();

            let mut line_w_offset = 0;
            let mut line_w = 0;
            for bb in glyphs.iter().filter_map(|g| g.pixel_bounding_box()) {
                line_w_offset = line_w_offset.max(-bb.min.x);
                line_w = line_w.max(bb.max.x + 1);
                block_h_offset = block_h_offset.max(-bb.min.y);
                block_h = block_h.max(bb.max.y + 1);
            }
            line_w += line_w_offset;
            line_glyphs.push((line_w as usize, line_w_offset, glyphs));

            block_w = block_w.max(line_w);
        }
        block_h += block_h_offset;
        let block_w = block_w as usize;
        let block_h = block_h as usize;

        // blit all the glyphs onto a bitmap
        let mut pixels = vec![false; block_w * block_h];
        for (line_w, line_w_offset, glyphs) in line_glyphs {
            for glyph in glyphs {
                if let Some(bb) = glyph.pixel_bounding_box() {
                    glyph.draw(|x, y, v| {
                        if v > 0.5 {
                            let x_offset = match alignment {
                                Alignment::Left => 0,
                                Alignment::Center => (block_w - line_w) / 2,
                                Alignment::Right => block_w - line_w,
                            } as i32;
                            let px = (x as i32 + line_w_offset + bb.min.x + x_offset) as usize;
                            let py = (y as i32 + block_h_offset + bb.min.y) as usize;
                            pixels[py * block_w + px] = true;
                        }
                    })
                }
            }
        }
        Bitmap {
            w: block_w,
            h: block_h,
            pixels,
        }
    }
    fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> Bitmap {
        let mut pixels = Vec::<bool>::with_capacity(w * h);
        for ny in 0..h {
            for nx in 0..w {
                pixels.push(self.pixels[(ny + y) * self.w + (nx + x)]);
            }
        }
        Bitmap { w, h, pixels }
    }
}
impl Drawable {
    fn from_bitmap(bitmap: Bitmap, x: DrawPos, y: DrawPos) -> Drawable {
        Drawable {
            x: match x {
                DrawPos::Coord(p) => p,
                DrawPos::Center => (SCREEN_WIDTH as isize - bitmap.w as isize) / 2,
            },
            y: match y {
                DrawPos::Coord(p) => p,
                DrawPos::Center => (SCREEN_HEIGHT as isize - bitmap.h as isize) / 2,
            },
            bitmap,
        }
    }
    fn rect(x: isize, y: isize, w: usize, h: usize, on: bool) -> Drawable {
        Drawable {
            x,
            y,
            bitmap: Bitmap::new(w, h, on),
        }
    }
    fn crop_to_screen(&self) -> Drawable {
        let src_x = max(-self.x, 0) as usize;
        let src_y = max(-self.y, 0) as usize;
        let dst_x = min(SCREEN_WIDTH - 1, max(self.x, 0) as usize);
        let dst_y = min(SCREEN_HEIGHT - 1, max(self.y, 0) as usize);
        let dst_w = min(self.bitmap.w, max(0, SCREEN_WIDTH as isize - dst_x as isize) as usize);
        let dst_h = min(self.bitmap.h, max(0, SCREEN_HEIGHT as isize - dst_y as isize) as usize);
        let src_w = min(dst_w, max(0, self.bitmap.w as isize - src_x as isize) as usize);
        let src_h = min(dst_h, max(0, self.bitmap.h as isize - src_y as isize) as usize);
        Drawable {
            x: dst_x as isize,
            y: dst_y as isize,
            bitmap: self.bitmap.crop(src_x, src_y, src_w, src_h),
        }
    }
    fn blit(&mut self, other: &Drawable) {
        for bx in 0..other.bitmap.w {
            for by in 0..other.bitmap.h {
                self.bitmap.set(
                    other.x + bx as isize,
                    other.y + by as isize,
                    other.bitmap.get(bx as isize, by as isize),
                );
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
            || (self.bitmap.w * self.bitmap.h <= 1024))
        {
            panic!("bitmap too large for one report");
        } else if self.bitmap.pixels.len() < self.bitmap.w * self.bitmap.h {
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
                let ri = x * stride_h + y;
                let pi = y * self.bitmap.w + x;
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
                x: self.x + (i * REPORT_SPLIT_SZ) as isize,
                y: self.y,
                bitmap: self.bitmap.crop(
                    i * REPORT_SPLIT_SZ,
                    0,
                    min(REPORT_SPLIT_SZ, self.bitmap.w - i * REPORT_SPLIT_SZ),
                    self.bitmap.h,
                ),
            });
        }
        vec
    }
}

#[derive(Clone, Copy)]
enum DrawPos {
    Coord(isize),
    Center,
}
impl FromStr for DrawPos {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<isize>().map(|n| Ok(DrawPos::Coord(n))).unwrap_or_else(|_| {
            if ["center", "c", "middle", "m"].contains(&s.to_lowercase().as_str()) {
                Ok(DrawPos::Center)
            } else {
                Err("not a valid position")
            }
        })
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Alignment {
    #[value(alias("l"))]
    Left,
    #[value(alias("c"))]
    Center,
    #[value(alias("r"))]
    Right,
}

#[derive(clap::Args)]
struct DrawArgs {
    #[arg(short = 'x', long, help = "Screen X offset for draw commands", default_value = "0")]
    screen_x: DrawPos,

    #[arg(short = 'y', long, help = "Screen Y offset for draw commands", default_value = "0")]
    screen_y: DrawPos,

    #[arg(
        short = 'C',
        long,
        help = "Clear the entire screen to black before drawing",
        default_value = "false"
    )]
    clear: bool,
    //
    // TODO: invert
    // TODO: autorefresh: redraw screen every ~3 seconds
    // TODO: oled screensaver: randomly change offset by ~2 pixels when drawing (may be janky)
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
#[command(about = "SteelSeries Arctis Nova Pro OLED drawing utility")]
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

        #[arg(short = 'a', long, help = "Text alignment", default_value = "left")]
        alignment: Alignment,

        #[arg(short = 'd', long, help = "Screen delimiter line for stdin input")]
        delimiter: Option<String>,
        //
        // TODO: custom font
        // TODO: font size
        // TODO: text scrolling (whole block or individual lines?) (may be jank)
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

        #[arg(
            short = 'r',
            long,
            help = "Frames to show per second (fps) - defaults to 1 fps or embedded delays for gif files"
        )]
        framerate: Option<u32>,

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

fn decode_frames(path: &str, image_args: &ImageArgs, draw_args: &DrawArgs) -> Vec<(Drawable, Option<Duration>)> {
    let reader = ImageReader::open(path).expect("Failed to open image");
    if matches!(reader.format().unwrap(), ImageFormat::Gif) {
        let gif = GifDecoder::new(reader.into_inner()).expect("Failed to decode gif");
        let frames = gif.into_frames();
        frames
            .map(|frame| {
                let frame = frame.expect("Failed to decode gif frame");
                let bitmap = Bitmap::from_image(frame.buffer(), image_args.threshold);
                let drawable = Drawable::from_bitmap(bitmap, draw_args.screen_x, draw_args.screen_y).crop_to_screen();
                (
                    drawable,
                    Some(Duration::from_millis(frame.delay().numer_denom_ms().0 as u64)),
                )
            })
            .collect()
    } else {
        let img = reader.decode().expect("Failed to decode image");
        let bitmap = Bitmap::from_dynimage(&img, image_args.threshold);
        let drawable = Drawable::from_bitmap(bitmap, draw_args.screen_x, draw_args.screen_y).crop_to_screen();
        vec![(drawable, None)]
    }
}

fn main() {
    let args = Args::parse();

    let api = hidapi::HidApi::new().unwrap();
    let dev = api
        .device_list()
        .find(|d| {
            d.vendor_id() == 0x1038 // SteelSeries
        && [
            0x12cb, // Arctis Nova Pro Wired
            0x12e0, // Arctis Nova Pro Wireless
        ].contains(&d.product_id()) && d.interface_number() == 4
        })
        .expect("Device not found")
        .open_device(&api)
        .expect("Failed to open device");

    match args {
        Args::Clear => draw(&dev, &Drawable::rect(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT, false), false),
        Args::Fill => draw(&dev, &Drawable::rect(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT, true), false),
        Args::Text {
            text,
            draw_args,
            alignment,
            delimiter,
        } => {
            let set_text = |text: &str| {
                let bitmap = Bitmap::from_text(&text, alignment);
                let drawable = Drawable::from_bitmap(bitmap, draw_args.screen_x, draw_args.screen_y).crop_to_screen();
                draw(&dev, &drawable, draw_args.clear);
            };
            if let Some(text) = text {
                // draw text to screen directly
                set_text(&text);
            } else {
                // iterate each line in stdin and draw to screen either when reaching EOF or when encountering `delimiter`
                let mut lines = vec![];
                for line in stdin().lines() {
                    let line = line.expect("Failed to read from stdin").replace('\r', "");
                    if Some(&line) == delimiter.as_ref() {
                        set_text(&lines.join("\n"));
                        lines.clear();
                    } else {
                        lines.push(line);
                    }
                }
                if lines.len() > 0 {
                    set_text(&lines.join("\n"));
                }
            }
        }
        Args::Img { path, image_args } => {
            let draw_args = &image_args.draw_args;
            let drawable = if path == "-" {
                let mut buf = Vec::<u8>::new();
                stdin().read_to_end(&mut buf).expect("Failed to read from stdin");
                let img = image::load_from_memory(&buf).expect("Failed to load image from stdin");
                let bitmap = Bitmap::from_dynimage(&img, image_args.threshold);
                Drawable::from_bitmap(bitmap, draw_args.screen_x, draw_args.screen_y).crop_to_screen()
            } else {
                let mut frames = decode_frames(&path, &image_args, &draw_args);
                if frames.len() != 1 {
                    eprintln!("img only supports images with single frame");
                }
                frames.swap_remove(0).0
            };
            draw(&dev, &drawable, draw_args.clear);
        }
        Args::Anim {
            framerate,
            loops,
            paths,
            image_args,
        } => {
            let draw_args = &image_args.draw_args;
            if framerate == Some(0) {
                panic!("Framerate must be non-zero");
            } else if paths.is_empty() {
                panic!("No image paths");
            }
            let period = framerate.map(|f| Duration::from_secs(1).div(f));
            let drawables: Vec<(Drawable, Duration)> = paths
                .iter()
                .flat_map(|path| {
                    decode_frames(&path, &image_args, &draw_args)
                        .into_iter()
                        .map(|(f, d)| (f, period.unwrap_or(d.unwrap_or(Duration::from_secs(1)))))
                })
                .collect();
            let mut frame_idx = 0;
            let mut draw_animation = || {
                for (drawable, delay) in &drawables {
                    let now_time = SystemTime::now();
                    let next_frame = now_time + *delay;
                    // TODO: handle clear properly when animation has varying image sizes
                    draw(&dev, drawable, frame_idx == 0);
                    frame_idx += 1;
                    if now_time < next_frame {
                        std::thread::sleep(next_frame.duration_since(SystemTime::now()).unwrap());
                    } else {
                        println!("fell behind - framerate too fast");
                    }
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
