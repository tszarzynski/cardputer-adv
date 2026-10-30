//! Screenshots as one line of text each, for an app to log over USB: a
//! [`Frame`]'s pixels, run-length encoded, in base64. `tools/screenshots.py`
//! turns a log of them into PNGs and a GIF.
//!
//! A line is `cardputer-screenshot 240x135 <ms> <base64>`: the size, the
//! time it was taken (milliseconds since the chip started), and the
//! pixels row by row as runs of one colour, each four bytes, big-endian:
//! the run's length in pixels, then its RGB565 colour.

use core::{fmt, str};

use embedded_graphics_core::{
    Pixel,
    draw_target::DrawTarget,
    geometry::Dimensions,
    pixelcolor::Rgb565,
    primitives::{PointsIter, Rectangle},
};

use crate::{Frame, HEIGHT, WIDTH};

/// What a screenshot's line starts with, for finding it in a log.
pub const SCREENSHOT_MARKER: &str = "cardputer-screenshot";

/// A run's length is a `u16`: the whole screen fits one.
const _: () = assert!(WIDTH as usize * HEIGHT as usize <= u16::MAX as usize);

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// A screenshot of a [`Frame`], from [`Frame::screenshot`], to log.
///
/// It formats ([`Display`](fmt::Display)) as one line of text. A screen of
/// small text comes to about 20 kB, each letter's edges breaking up its
/// rows; a busy one, such as a photo, to more than the 87 kB of its raw
/// pixels in base64.
///
/// esp-println holds interrupts off while it prints a line, about 0.1 s
/// for 20 kB, so the speaker's and the microphone's streams stall and
/// restart while a screenshot is logged: they're for the screen, not for
/// sound. (Logging it in several lines would be no better: each line takes
/// tens of milliseconds over USB, however short.)
#[derive(Clone, Copy)]
pub struct Screenshot<'a> {
    /// As [`Frame`] stores them: each `u16`'s bytes in memory are the
    /// colour's, big-endian.
    pixels: &'a [u16],
    millis: u64,
}

impl<'a> Screenshot<'a> {
    pub(crate) const fn new(pixels: &'a [u16], millis: u64) -> Self {
        Self { pixels, millis }
    }

    /// The pixels as runs of one colour, each as its four bytes.
    fn runs(&self) -> impl Iterator<Item = u8> {
        self.pixels.chunk_by(|a, b| a == b).flat_map(|run| {
            let length = u16::try_from(run.len()).unwrap_or(u16::MAX);
            let [high, low] = length.to_be_bytes();
            let [first, second] = run[0].to_ne_bytes();
            [high, low, first, second]
        })
    }
}

impl fmt::Display for Screenshot<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{SCREENSHOT_MARKER} {WIDTH}x{HEIGHT} {} ", self.millis)?;
        write_base64(f, self.runs())
    }
}

impl fmt::Debug for Screenshot<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Screenshot")
            .field("millis", &self.millis)
            .finish_non_exhaustive()
    }
}

/// Writes `bytes` in base64, 1 kB at a time: each write to the log has a
/// cost of its own (64 characters at a time sent a sixth as fast over
/// USB).
fn write_base64(f: &mut impl fmt::Write, bytes: impl Iterator<Item = u8>) -> fmt::Result {
    let mut chunk = [0; 1024];
    let mut len = 0;
    let mut bytes = bytes.peekable();
    while bytes.peek().is_some() {
        let group = [bytes.next(), bytes.next(), bytes.next()];
        let [a, b, c] = group.map(|byte| byte.unwrap_or(0));
        let bits = u32::from(a) << 16 | u32::from(b) << 8 | u32::from(c);
        // Two characters for one byte, three for two, four for three.
        let used = 1 + group.iter().flatten().count();
        for i in 0..4 {
            chunk[len + i] = if i < used {
                BASE64[(bits >> (18 - 6 * i) & 63) as usize]
            } else {
                b'='
            };
        }
        len += 4;
        if len == chunk.len() {
            f.write_str(str::from_utf8(&chunk).unwrap_or_default())?;
            len = 0;
        }
    }
    f.write_str(str::from_utf8(&chunk[..len]).unwrap_or_default())
}

/// A draw target that draws to `target` and into a [`Frame`] as well.
///
/// For screenshots of an app that draws straight to the [`Display`] rather
/// than through a [`FrameDisplay`]: draw through it, then log
/// `frame.screenshot()`.
///
/// ```ignore
/// static FRAME: ConstStaticCell<Frame> = ConstStaticCell::new(Frame::new());
///
/// let frame = FRAME.take();
/// text.draw(&mut Mirror::new(&mut display, frame))?;
/// info!("{}", frame.screenshot());
/// ```
///
/// [`Display`]: crate::Display
/// [`FrameDisplay`]: crate::FrameDisplay
pub struct Mirror<'a, D> {
    target: &'a mut D,
    frame: &'a mut Frame,
}

impl<'a, D> Mirror<'a, D> {
    /// Draws to `target`, and into `frame` too.
    pub const fn new(target: &'a mut D, frame: &'a mut Frame) -> Self {
        Self { target, frame }
    }
}

impl<D: Dimensions> Dimensions for Mirror<'_, D> {
    fn bounding_box(&self) -> Rectangle {
        self.target.bounding_box()
    }
}

impl<D: DrawTarget<Color = Rgb565>> DrawTarget for Mirror<'_, D> {
    type Color = Rgb565;
    type Error = D::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), D::Error>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        let frame = &mut *self.frame;
        self.target.draw_iter(pixels.into_iter().inspect(|&pixel| {
            let Ok(()) = frame.draw_iter([pixel]);
        }))
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), D::Error>
    where
        I: IntoIterator<Item = Rgb565>,
    {
        let frame = &mut *self.frame;
        let colors = area.points().zip(colors).map(|(point, color)| {
            let Ok(()) = frame.draw_iter([Pixel(point, color)]);
            color
        });
        self.target.fill_contiguous(area, colors)
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Rgb565) -> Result<(), D::Error> {
        let Ok(()) = self.frame.fill_solid(area, color);
        self.target.fill_solid(area, color)
    }

    fn clear(&mut self, color: Rgb565) -> Result<(), D::Error> {
        let Ok(()) = self.frame.clear(color);
        self.target.clear(color)
    }
}

#[cfg(test)]
mod tests {
    use embedded_graphics_core::{
        geometry::{OriginDimensions, Point, Size},
        pixelcolor::{IntoStorage, RgbColor},
    };

    use super::*;

    /// The bytes in a line's base64.
    fn unbase64(text: &str) -> Vec<u8> {
        let sextets: Vec<u32> = text
            .bytes()
            .filter(|&c| c != b'=')
            .map(|c| BASE64.iter().position(|&b| b == c).expect("base64"))
            .map(|s| u32::try_from(s).expect("< 64"))
            .collect();
        sextets
            .chunks(4)
            .flat_map(|group| {
                let bits = group
                    .iter()
                    .enumerate()
                    .fold(0, |bits, (i, &s)| bits | s << (18 - 6 * i));
                bits.to_be_bytes()[1..group.len()].to_vec()
            })
            .collect()
    }

    /// The time and the pixels (RGB565 values) in a screenshot's line, as
    /// `tools/screenshots.py` reads them.
    fn decode(line: &str) -> (u64, Vec<u16>) {
        let fields: Vec<&str> = line.split(' ').collect();
        let [marker, size, ms, text] = fields[..] else {
            panic!("four fields: {line}");
        };
        assert_eq!((marker, size), (SCREENSHOT_MARKER, "240x135"));
        let pixels = unbase64(text)
            .chunks(4)
            .flat_map(|run| {
                let length = u16::from_be_bytes([run[0], run[1]]);
                let color = u16::from_be_bytes([run[2], run[3]]);
                std::iter::repeat_n(color, length.into())
            })
            .collect();
        (ms.parse().expect("ms"), pixels)
    }

    fn line(frame: &Frame, millis: u64) -> String {
        Screenshot::new(frame.pixels(), millis).to_string()
    }

    #[test]
    fn a_screenshot_decodes_to_the_frame() {
        let mut frame = Frame::new();
        let Ok(()) = frame.clear(Rgb565::BLUE);
        let square = Rectangle::new(Point::new(10, 20), Size::new(30, 40));
        let Ok(()) = frame.fill_solid(&square, Rgb565::RED);
        let Ok(()) = frame.draw_iter([Pixel(Point::new(239, 134), Rgb565::GREEN)]);
        let (millis, pixels) = decode(&line(&frame, 1234));
        assert_eq!(millis, 1234);
        assert_eq!(pixels.len(), 240 * 135);
        let at = |x: usize, y: usize| pixels[y * 240 + x];
        assert_eq!(at(0, 0), Rgb565::BLUE.into_storage());
        assert_eq!(at(10, 20), Rgb565::RED.into_storage());
        assert_eq!(at(39, 59), Rgb565::RED.into_storage());
        assert_eq!(at(40, 59), Rgb565::BLUE.into_storage());
        assert_eq!(at(239, 134), Rgb565::GREEN.into_storage());
    }

    #[test]
    fn one_colour_is_one_run() {
        let mut frame = Frame::new();
        let Ok(()) = frame.clear(Rgb565::WHITE);
        let line = line(&frame, 0);
        // A run's four bytes: six characters, and two of padding.
        assert_eq!(line.rsplit(' ').next().map(str::len), Some(8));
        let (_, pixels) = decode(&line);
        assert!(pixels.iter().all(|&p| p == Rgb565::WHITE.into_storage()));
    }

    #[test]
    fn a_busy_screen_decodes_too() {
        // Stripes a pixel wide: a run each, 32,400 of them.
        let mut frame = Frame::new();
        let stripes = (0..240)
            .step_by(2)
            .flat_map(|x| (0..135).map(move |y| Pixel(Point::new(x, y), Rgb565::WHITE)));
        let Ok(()) = frame.draw_iter(stripes);
        let (_, pixels) = decode(&line(&frame, 7));
        assert_eq!(pixels.len(), 240 * 135);
        assert_eq!(pixels[2], Rgb565::WHITE.into_storage());
        assert_eq!(pixels[3], Rgb565::BLACK.into_storage());
    }

    #[test]
    fn base64_pads_as_the_standard_says() {
        let encode = |bytes: &[u8]| {
            let mut text = String::new();
            write_base64(&mut text, bytes.iter().copied()).expect("a String");
            text
        };
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(&[0xAB; 100]).len(), 136);
    }

    /// A draw target that keeps nothing but what it was asked to draw.
    struct Canvas(Vec<u16>);

    impl OriginDimensions for Canvas {
        fn size(&self) -> Size {
            Size::new(240, 135)
        }
    }

    impl DrawTarget for Canvas {
        type Color = Rgb565;
        type Error = core::convert::Infallible;

        fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Rgb565>>,
        {
            for Pixel(point, color) in pixels {
                if let (Ok(x), Ok(y)) = (usize::try_from(point.x), usize::try_from(point.y))
                    && x < 240
                    && y < 135
                {
                    self.0[y * 240 + x] = color.into_storage();
                }
            }
            Ok(())
        }
    }

    #[test]
    fn a_mirror_draws_the_same_to_both() {
        let mut canvas = Canvas(vec![0; 240 * 135]);
        let mut frame = Frame::new();
        let mut mirror = Mirror::new(&mut canvas, &mut frame);
        let Ok(()) = mirror.clear(Rgb565::BLACK);
        let area = Rectangle::new(Point::new(5, 5), Size::new(3, 2));
        let Ok(()) =
            mirror.fill_contiguous(&area, [Rgb565::RED, Rgb565::GREEN].into_iter().cycle());
        let Ok(()) = mirror.fill_solid(
            &Rectangle::new(Point::new(100, 50), Size::new(20, 20)),
            Rgb565::YELLOW,
        );
        let Ok(()) = mirror.draw_iter([Pixel(Point::new(200, 100), Rgb565::CYAN)]);
        let (_, pixels) = decode(&line(&frame, 0));
        assert_eq!(pixels, canvas.0);
        assert_eq!(canvas.0[5 * 240 + 6], Rgb565::GREEN.into_storage());
    }
}
