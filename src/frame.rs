//! The whole screen in memory, and which parts of it have changed since
//! they were sent.

use core::{
    convert::Infallible,
    fmt,
    ops::{Range, RangeInclusive},
};

use embedded_graphics_core::{
    Pixel,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::{IntoStorage, Rgb565},
    primitives::Rectangle,
};

use crate::{HEIGHT, WIDTH};

const W: usize = WIDTH as usize;
const H: usize = HEIGHT as usize;

/// The columns of a row written since the last flush, `first..=last`.
/// Clean when `first > last`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    first: u8,
    last: u8,
}

impl Span {
    const CLEAN: Self = Self {
        first: u8::MAX,
        last: 0,
    };
    #[expect(clippy::cast_possible_truncation, reason = "the width (240) fits")]
    const FULL: Self = Self {
        first: 0,
        last: (W - 1) as u8,
    };

    const fn is_clean(self) -> bool {
        self.first > self.last
    }

    /// The span, widened to take in column `x`.
    #[expect(clippy::cast_possible_truncation, reason = "x < WIDTH (240)")]
    fn with(self, x: usize) -> Self {
        let x = x as u8;
        Self {
            first: self.first.min(x),
            last: self.last.max(x),
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            first: self.first.min(other.first),
            last: self.last.max(other.last),
        }
    }
}

/// A row's fingerprint: tells whether it differs from the row last sent.
/// Each step is a bijection of the running value, so a change to one
/// pair of pixels always shows; two changes cancelling out is a 1 in 2³²
/// chance.
fn row_hash(row: &[u16]) -> u32 {
    let (pairs, _) = row.as_chunks::<2>();
    pairs.iter().fold(0x811C_9DC5, |hash, &[a, b]| {
        (hash ^ (u32::from(a) | u32::from(b) << 16))
            .wrapping_mul(0x0100_0193)
            .rotate_left(5)
    })
}

/// A rectangle of changed rows to send: `rows`, each `columns` wide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// The rows, top to bottom.
    pub rows: Range<usize>,
    /// The columns, as wide as what was drawn on any of the rows.
    pub columns: RangeInclusive<usize>,
}

impl Run {
    /// Whether the run is whole rows, so its pixels are one slice.
    pub const fn is_full_width(&self) -> bool {
        *self.columns.start() == 0 && *self.columns.end() == W - 1
    }
}

/// The whole screen in memory (64.8 kB of RGB565), for [`FrameDisplay`].
///
/// Draw into it with embedded-graphics, overlapping shapes freely; then
/// [`FrameDisplay::flush`] sends only the rows that changed since the last
/// flush. Rows are compared with what was sent, so redrawing everything
/// each frame is fine: rows that come out the same aren't sent again.
///
/// It's too big for a task's stack; keep it in a static:
///
/// ```ignore
/// static FRAME: ConstStaticCell<Frame> = ConstStaticCell::new(Frame::new());
/// ```
///
/// [`FrameDisplay`]: crate::FrameDisplay
/// [`FrameDisplay::flush`]: crate::FrameDisplay::flush
pub struct Frame {
    /// RGB565, row by row, each stored so that its bytes in memory are
    /// big-endian: the bytes the panel takes. Whole `u16`s, so filling
    /// takes one store a pixel.
    pixels: [u16; W * H],
    /// What each row has had drawn on it since the last flush.
    written: [Span; H],
    /// Each row's [`row_hash`] when it was last sent.
    sent: [u32; H],
    /// Each row's hash as [`next_run`](Self::next_run) found it, to become
    /// [`sent`](Self::sent) once the row is.
    pending: [u32; H],
    /// Send every written row, whatever its hash: the panel's contents
    /// aren't known.
    stale: bool,
}

impl Frame {
    /// A black frame, all of it still to be sent (the panel starts with
    /// whatever its memory held).
    #[must_use]
    #[expect(
        clippy::large_stack_arrays,
        reason = "made at compile time, into a static (see above)"
    )]
    pub const fn new() -> Self {
        Self {
            pixels: [0; W * H],
            written: [Span::FULL; H],
            sent: [0; H],
            pending: [0; H],
            stale: true,
        }
    }

    /// Marks the whole frame to be sent again, for when the panel's
    /// contents were lost or overwritten.
    pub const fn invalidate(&mut self) {
        self.written = [Span::FULL; H];
        self.stale = true;
    }

    /// The next run of rows to send, from row `from` on: rows that were
    /// written and differ from what was sent, as one rectangle as wide as
    /// what was drawn on them. Rows found unchanged on the way are marked
    /// clean.
    pub(crate) fn next_run(&mut self, from: usize) -> Option<Run> {
        let mut y = from;
        while y < H {
            let top = y;
            let mut span = Span::CLEAN;
            while y < H
                && let Some(hash) = self.changed(y)
            {
                self.pending[y] = hash;
                span = span.union(self.written[y]);
                y += 1;
            }
            if !span.is_clean() {
                return Some(Run {
                    rows: top..y,
                    columns: usize::from(span.first)..=usize::from(span.last),
                });
            }
            y += 1;
        }
        None
    }

    /// Marks `run` as sent: its rows are clean until drawn on again.
    pub(crate) fn mark_sent(&mut self, run: &Run) {
        let rows = run.rows.clone();
        self.sent[rows.clone()].copy_from_slice(&self.pending[rows.clone()]);
        self.written[rows].fill(Span::CLEAN);
    }

    /// Marks the panel as showing the frame: from now on, rows written
    /// back the way they were aren't sent.
    pub(crate) const fn mark_shown(&mut self) {
        self.stale = false;
    }

    /// Row `row`'s pixels in `columns`, as the bytes the panel takes.
    pub(crate) fn row_bytes(&self, row: usize, columns: RangeInclusive<usize>) -> &[u8] {
        let (first, last) = columns.into_inner();
        bytemuck::cast_slice(&self.pixels[row * W + first..=row * W + last])
    }

    /// Whole `rows`' pixels, as the bytes the panel takes.
    pub(crate) fn rows_bytes(&self, rows: Range<usize>) -> &[u8] {
        bytemuck::cast_slice(&self.pixels[rows.start * W..rows.end * W])
    }

    /// Row `y`'s hash, if it has to be sent: it was written, and it differs
    /// from what was sent. A row written back the way it was is marked
    /// clean.
    fn changed(&mut self, y: usize) -> Option<u32> {
        if self.written[y].is_clean() {
            return None;
        }
        let hash = row_hash(&self.pixels[y * W..(y + 1) * W]);
        if !self.stale && hash == self.sent[y] {
            self.written[y] = Span::CLEAN;
            return None;
        }
        Some(hash)
    }

    /// `color` as [`pixels`](Self::pixels) stores it.
    fn raw(color: Rgb565) -> u16 {
        u16::from_ne_bytes(color.into_storage().to_be_bytes())
    }

    /// Sets row `y`'s pixels `x0..=x1` to `raw`.
    fn fill_row(&mut self, y: usize, x0: usize, x1: usize, raw: u16) {
        // Two pixels a store where the row is word-aligned: memory writes
        // are what filling costs.
        let row = &mut self.pixels[y * W + x0..=y * W + x1];
        let (head, words, tail) = bytemuck::pod_align_to_mut::<u16, u32>(row);
        head.fill(raw);
        words.fill(u32::from(raw) * 0x0001_0001);
        tail.fill(raw);
        self.written[y] = self.written[y].with(x0).with(x1);
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

impl OriginDimensions for Frame {
    fn size(&self) -> Size {
        Size::new(WIDTH.into(), HEIGHT.into())
    }
}

impl DrawTarget for Frame {
    type Color = Rgb565;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Infallible>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        for Pixel(point, color) in pixels {
            if let (Ok(x), Ok(y)) = (usize::try_from(point.x), usize::try_from(point.y))
                && x < W
                && y < H
            {
                self.pixels[y * W + x] = Self::raw(color);
                self.written[y] = self.written[y].with(x);
            }
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Rgb565) -> Result<(), Infallible> {
        // Clipped to the frame: `start..end` in each direction.
        let clip = |start: i32, length: u32, limit: usize| {
            let end = start.saturating_add_unsigned(length);
            let clamp = |v: i32| usize::try_from(v).unwrap_or(0).min(limit);
            (clamp(start), clamp(end))
        };
        let (x0, x1) = clip(area.top_left.x, area.size.width, W);
        let (y0, y1) = clip(area.top_left.y, area.size.height, H);
        if x0 >= x1 {
            return Ok(());
        }
        let raw = Self::raw(color);
        for y in y0..y1 {
            self.fill_row(y, x0, x1 - 1, raw);
        }
        Ok(())
    }
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let written = self.written.iter().filter(|span| !span.is_clean()).count();
        f.debug_struct("Frame")
            .field("rows_written", &written)
            .field("stale", &self.stale)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use embedded_graphics_core::{geometry::Point, pixelcolor::RgbColor};

    use super::*;

    /// Every run a flush would send now, marking each sent.
    fn flush(frame: &mut Frame) -> Vec<Run> {
        let mut runs = Vec::new();
        let mut from = 0;
        while let Some(run) = frame.next_run(from) {
            from = run.rows.end;
            frame.mark_sent(&run);
            runs.push(run);
        }
        frame.mark_shown();
        runs
    }

    /// A frame the panel already shows.
    fn shown() -> Box<Frame> {
        let mut frame = Box::new(Frame::new());
        flush(&mut frame);
        frame
    }

    fn dot(frame: &mut Frame, x: i32, y: i32, color: Rgb565) {
        let Ok(()) = frame.draw_iter([Pixel(Point::new(x, y), color)]);
    }

    fn fill(frame: &mut Frame, x: i32, y: i32, size: u32, color: Rgb565) {
        let area = Rectangle::new(Point::new(x, y), Size::new(size, size));
        let Ok(()) = frame.fill_solid(&area, color);
    }

    const fn run(rows: Range<usize>, columns: RangeInclusive<usize>) -> Run {
        Run { rows, columns }
    }

    #[test]
    fn a_new_frame_is_sent_whole_then_nothing() {
        let mut frame = Box::new(Frame::new());
        let runs = flush(&mut frame);
        assert_eq!(runs, [run(0..H, 0..=W - 1)]);
        assert!(runs[0].is_full_width());
        assert_eq!(flush(&mut frame), []);
    }

    #[test]
    fn only_what_changed_is_sent() {
        let mut frame = shown();
        dot(&mut frame, 10, 5, Rgb565::RED);
        dot(&mut frame, 30, 6, Rgb565::RED);
        dot(&mut frame, 7, 9, Rgb565::RED);
        assert_eq!(flush(&mut frame), [run(5..7, 10..=30), run(9..10, 7..=7)]);
        assert_eq!(
            frame.row_bytes(5, 10..=10),
            Rgb565::RED.into_storage().to_be_bytes()
        );
    }

    #[test]
    fn a_row_drawn_back_the_way_it_was_isnt_sent() {
        let mut frame = shown();
        dot(&mut frame, 10, 5, Rgb565::BLACK);
        assert_eq!(flush(&mut frame), []);
        // Unless the panel's contents are unknown again.
        frame.invalidate();
        dot(&mut frame, 10, 5, Rgb565::BLACK);
        assert_eq!(flush(&mut frame), [run(0..H, 0..=W - 1)]);
    }

    #[test]
    fn a_run_not_sent_is_found_again() {
        let mut frame = shown();
        dot(&mut frame, 3, 3, Rgb565::GREEN);
        // A cancelled flush: found, but never marked sent.
        assert_eq!(frame.next_run(0), Some(run(3..4, 3..=3)));
        assert_eq!(flush(&mut frame), [run(3..4, 3..=3)]);
    }

    #[test]
    fn drawing_is_clipped_to_the_screen() {
        let mut frame = shown();
        fill(&mut frame, -5, -5, 10, Rgb565::BLUE);
        assert_eq!(flush(&mut frame), [run(0..5, 0..=4)]);
        fill(&mut frame, 300, 10, 10, Rgb565::BLUE);
        dot(&mut frame, -1, 200, Rgb565::BLUE);
        assert_eq!(flush(&mut frame), []);
    }

    #[test]
    fn whole_rows_are_one_slice() {
        let frame = Box::new(Frame::new());
        assert_eq!(frame.rows_bytes(2..4).len(), 2 * W * 2);
        assert_eq!(frame.row_bytes(2, 0..=9).len(), 20);
    }

    #[test]
    fn one_changed_pixel_changes_the_hash() {
        let mut row = [0u16; W];
        let before = row_hash(&row);
        row[W - 1] = 1;
        assert_ne!(row_hash(&row), before);
    }
}
