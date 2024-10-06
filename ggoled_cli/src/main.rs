use clap::{command, Parser, ValueEnum};
use core::str;
use ggoled_lib::Bitmap;
use ggoled_lib::{bitmap::BitVec, Device};
use image::{codecs::gif::GifDecoder, io::Reader as ImageReader, AnimationDecoder, ImageFormat};
use rusttype::{point, Font, Scale};
use spin_sleep::sleep;
use std::{
    cmp::{max, min},
    io::{stdin, Read},
    ops::Div,
    str::FromStr,
    sync::mpsc::channel,
    time::{Duration, SystemTime},
};

pub fn bitmap_from_image(img: &image::RgbaImage, threshold: usize) -> Bitmap {
    Bitmap {
        w: img.width() as usize,
        h: img.height() as usize,
        data: img
            .pixels()
            .map(|p| ((p.0[0] as usize) + (p.0[1] as usize) + (p.0[2] as usize)) / 3 >= threshold)
            .collect::<BitVec>(),
    }
}
pub fn bitmap_from_dynimage(img: &image::DynamicImage, threshold: usize) -> Bitmap {
    bitmap_from_image(&img.to_rgba8(), threshold)
}

struct TextRenderer {
    font: Font<'static>,
}
impl TextRenderer {
    pub fn new() -> Self {
        let font = Font::try_from_bytes(include_bytes!("../../fonts/PixelOperator.ttf")).unwrap();
        Self { font }
    }
    pub fn render(&self, text: &str, alignment: Alignment) -> Bitmap {
        let clean_text = text.replace('\r', "");
        let text_lines = clean_text.split('\n');

        let scale = Scale::uniform(16.0);
        let v_metrics = self.font.v_metrics(scale);
        let line_h = (v_metrics.ascent - v_metrics.descent).ceil();

        // collect all glyphs for each line of text and calculate bounds
        let mut line_glyphs = vec![];
        let mut block_w = 0;
        let mut block_h_offset = 0;
        let mut block_h = 0;
        for (yi, text_line) in text_lines.enumerate() {
            let glyphs: Vec<_> = self
                .font
                .layout(text_line, scale, point(0.0, yi as f32 * line_h))
                .collect();

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
        let mut bitmap = Bitmap::new(block_w, block_h, false);
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
                            bitmap.data.set(py * block_w + px, true);
                        }
                    })
                }
            }
        }
        bitmap
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
            if ["center", "c"].contains(&s.to_lowercase().as_str()) {
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

#[derive(Clone, Copy, ValueEnum)]
enum ScrollSpeed {
    Slow,
    Normal,
    Fast,
}

#[derive(clap::Args)]
struct DrawArgs {
    #[arg(
        short = 'x',
        long,
        help = "Screen X offset for draw commands",
        default_value = "center"
    )]
    screen_x: DrawPos,

    #[arg(
        short = 'y',
        long,
        help = "Screen Y offset for draw commands",
        default_value = "center"
    )]
    screen_y: DrawPos,

    #[arg(short = 'n', long, help = "Don't clear the screen to before drawing")]
    no_clear: bool,
    //
    // TODO: invert
    // TODO: autorefresh: redraw screen every ~3 seconds (when in daemon mode)
    // TODO: oled screensaver: randomly change offset by ~2 pixels every few minutes when drawing (when in daemon mode)
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

        #[arg(short = 'a', long, help = "Text alignment", default_value = "center")]
        alignment: Alignment,

        #[arg(short = 's', long, help = "Text scrolling")]
        scroll: Option<ScrollSpeed>,

        #[arg(short = 'd', long, help = "Screen delimiter line for stdin input")]
        delimiter: Option<String>,
        //
        // TODO: custom font
        // TODO: font size
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

fn decode_frames(path: &str, image_args: &ImageArgs) -> Vec<(Bitmap, Option<Duration>)> {
    let reader = ImageReader::open(path).expect("Failed to open image");
    if matches!(reader.format().unwrap(), ImageFormat::Gif) {
        let gif = GifDecoder::new(reader.into_inner()).expect("Failed to decode gif");
        let frames = gif.into_frames();
        frames
            .map(|frame| {
                let frame = frame.expect("Failed to decode gif frame");
                let bitmap = bitmap_from_image(frame.buffer(), image_args.threshold);
                (
                    bitmap,
                    Some(Duration::from_millis(frame.delay().numer_denom_ms().0 as u64)),
                )
            })
            .collect()
    } else {
        let img = reader.decode().expect("Failed to decode image");
        let bitmap = bitmap_from_dynimage(&img, image_args.threshold);
        vec![(bitmap, None)]
    }
}

fn draw_with_args(dev: &Device, bitmap: &Bitmap, draw: &DrawArgs) {
    let x = match draw.screen_x {
        DrawPos::Coord(v) => v,
        DrawPos::Center => (dev.width as isize - bitmap.w as isize) / 2,
    };
    let y = match draw.screen_y {
        DrawPos::Coord(v) => v,
        DrawPos::Center => (dev.height as isize - bitmap.h as isize) / 2,
    };
    if draw.no_clear {
        dev.draw(&bitmap, x, y).unwrap();
    } else {
        let mut screen = Bitmap::new(dev.width, dev.height, false);
        screen.blit(&bitmap, x, y);
        dev.draw(&screen, 0, 0).unwrap();
    };
}

fn main() {
    let args = Args::parse();
    let dev = Device::connect().unwrap();

    match args {
        Args::Clear => dev.draw(&Bitmap::new(dev.width, dev.height, false), 0, 0).unwrap(),
        Args::Fill => dev.draw(&Bitmap::new(dev.width, dev.height, true), 0, 0).unwrap(),
        Args::Text {
            text,
            draw_args,
            alignment,
            delimiter,
            scroll,
        } => {
            let (ch_send, ch_recv) = channel::<Option<String>>();
            let animate = scroll.is_some();
            let animate_wait = scroll.map(|s| match s {
                ScrollSpeed::Slow => Duration::from_millis(100),
                ScrollSpeed::Normal => Duration::from_millis(50),
                ScrollSpeed::Fast => Duration::from_millis(20),
            });
            let text_thread_fn = move || {
                let rnd = TextRenderer::new();
                if animate {
                    let mut current_bitmap: Option<Bitmap> = None;
                    let mut current_x: isize = 0;
                    let mut start_x: isize = 0;
                    let mut end_x: isize = 0;
                    let mut first_draw = false;
                    const X_WAIT: isize = 10; // used for hack to wait at start and end
                    loop {
                        match ch_recv.try_recv() {
                            Ok(Some(text)) => {
                                let bitmap = rnd.render(&text, alignment);
                                let margin_x = match draw_args.screen_x {
                                    DrawPos::Coord(x) => {
                                        start_x = x;
                                        x
                                    }
                                    DrawPos::Center => {
                                        start_x = max(0, (dev.width as isize - bitmap.w as isize) / 2);
                                        0
                                    }
                                };
                                end_x = min(start_x, (dev.width as isize - margin_x) - bitmap.w as isize);
                                current_x = start_x + X_WAIT;
                                current_bitmap = Some(bitmap);
                                first_draw = true;
                            }
                            Ok(None) => {
                                break;
                            }
                            Err(_) => {} // no data
                        }
                        if let Some(bitmap) = &current_bitmap {
                            let last_x = current_x;
                            current_x -= 1;
                            if current_x <= end_x - X_WAIT {
                                current_x = start_x + X_WAIT;
                            }
                            if current_x != last_x || first_draw {
                                draw_with_args(
                                    &dev,
                                    bitmap,
                                    &DrawArgs {
                                        screen_x: DrawPos::Coord(max(min(current_x, start_x), end_x)),
                                        ..draw_args
                                    },
                                );
                                first_draw = false;
                            }
                        }
                        sleep(animate_wait.unwrap());
                    }
                } else {
                    while let Some(text) = ch_recv.recv().unwrap() {
                        let bitmap = rnd.render(&text, alignment);
                        draw_with_args(&dev, &bitmap, &draw_args);
                    }
                }
            };
            let text_thread = std::thread::spawn(text_thread_fn);
            if let Some(text) = text {
                // draw text to screen directly
                ch_send.send(Some(text)).unwrap();
                if !animate {
                    ch_send.send(None).unwrap(); // stop thread if not animating
                }
            } else {
                // iterate each line in stdin and draw to screen either when reaching EOF or when encountering `delimiter`
                let mut lines = vec![];
                for line in stdin().lines() {
                    let line = line.expect("Failed to read from stdin").replace('\r', "");
                    if Some(&line) == delimiter.as_ref() {
                        ch_send.send(Some(lines.join("\n"))).unwrap();
                        lines.clear();
                    } else {
                        lines.push(line);
                    }
                }
                if lines.len() > 0 {
                    ch_send.send(Some(lines.join("\n"))).unwrap();
                }
                ch_send.send(None).unwrap(); // stop thread on EOF
            }
            text_thread.join().unwrap();
        }
        Args::Img { path, image_args } => {
            let bitmap = if path == "-" {
                let mut buf = Vec::<u8>::new();
                stdin().read_to_end(&mut buf).expect("Failed to read from stdin");
                let img = image::load_from_memory(&buf).expect("Failed to load image from stdin");
                bitmap_from_dynimage(&img, image_args.threshold)
            } else {
                let mut frames = decode_frames(&path, &image_args);
                if frames.len() != 1 {
                    eprintln!("img only supports images with single frame");
                }
                frames.swap_remove(0).0
            };
            draw_with_args(&dev, &bitmap, &image_args.draw_args);
        }
        Args::Anim {
            framerate,
            loops,
            paths,
            image_args,
        } => {
            if framerate == Some(0) {
                panic!("Framerate must be non-zero");
            } else if paths.is_empty() {
                panic!("No image paths");
            }
            let period = framerate.map(|f| Duration::from_secs(1).div(f));
            let bitmaps: Vec<(Bitmap, Duration)> = paths
                .iter()
                .flat_map(|path| {
                    decode_frames(&path, &image_args)
                        .into_iter()
                        .map(|(f, d)| (f, period.unwrap_or(d.unwrap_or(Duration::from_secs(1)))))
                })
                .collect();
            let mut frame_idx = 0;
            let mut draw_animation = || {
                for (bitmap, delay) in &bitmaps {
                    let now_time = SystemTime::now();
                    let next_frame = now_time + *delay;
                    draw_with_args(&dev, &bitmap, &image_args.draw_args);
                    frame_idx += 1;
                    if now_time < next_frame {
                        sleep(next_frame.duration_since(SystemTime::now()).unwrap());
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
