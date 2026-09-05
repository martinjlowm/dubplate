//! The spectrogram: a PNG of the pixel data with an SVG wrapper carrying the
//! axes.
//!
//! Split in two because the two halves want different formats. A megapixel of
//! magnitudes is a raster, and drawing it as SVG rectangles produces a file no
//! browser will open; axis labels want to stay text. The wrapper references the
//! PNG beside it, so both files travel together in the output directory.

use crate::plot::escape;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

const MARGIN_LEFT: f64 = 68.0;
const MARGIN_RIGHT: f64 = 22.0;
const MARGIN_TOP: f64 = 34.0;
const MARGIN_BOTTOM: f64 = 46.0;

/// A time-frequency image. Row 0 is the highest frequency, as drawn.
pub struct Heatmap {
    pub title: String,
    pub width: usize,
    pub height: usize,
    /// Row-major decibel values, `width * height` of them.
    pub values: Vec<f32>,
    pub floor_db: f32,
    pub ceiling_db: f32,
    /// Seconds at the left and right edge.
    pub time_range: (f64, f64),
    /// Hertz at the bottom and top edge, spaced logarithmically.
    pub frequency_range: (f64, f64),
}

impl Heatmap {
    /// The PNG as bytes.
    ///
    /// Bytes rather than a file, because the same encoder runs in a browser
    /// where there is nowhere to put a path. `write_png` is this plus a write.
    pub fn to_png_bytes(&self) -> io::Result<Vec<u8>> {
        let mut out = Vec::with_capacity(self.width * self.height);
        {
            let mut encoder = png::Encoder::new(&mut out, self.width as u32, self.height as u32);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header()?;

            let span = (self.ceiling_db - self.floor_db).max(f32::MIN_POSITIVE);
            let mut pixels = Vec::with_capacity(self.width * self.height * 3);
            for value in &self.values {
                let level = ((value - self.floor_db) / span).clamp(0.0, 1.0);
                let [r, g, b] = colour(level);
                pixels.extend_from_slice(&[r, g, b]);
            }
            writer.write_image_data(&pixels)?;
        }
        Ok(out)
    }

    pub fn write_png(&self, path: &Path) -> io::Result<()> {
        let mut file = BufWriter::new(File::create(path)?);
        file.write_all(&self.to_png_bytes()?)
    }

    /// An SVG that draws the axes around `png_href`, which is resolved relative
    /// to the SVG file.
    pub fn to_svg(&self, png_href: &str) -> String {
        let width = 960.0;
        let height = 420.0;
        let plot_width = width - MARGIN_LEFT - MARGIN_RIGHT;
        let plot_height = height - MARGIN_TOP - MARGIN_BOTTOM;

        let mut svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" class="dubplate-figure" width="{width}" height="{height}" viewBox="0 0 {width} {height}" font-family="ui-sans-serif, system-ui, sans-serif">
<style>.dubplate-figure .tick{{font-size:11px;fill:#475569}}.dubplate-figure .axis{{stroke:#94a3b8;stroke-width:1}}.dubplate-figure .title{{font-size:14px;fill:#0f172a;font-weight:600}}.dubplate-figure .label{{font-size:12px;fill:#334155}}</style>
<rect width="{width}" height="{height}" fill="white"/>
<text x="{MARGIN_LEFT}" y="20" class="title">{}</text>
<image href="{}" x="{MARGIN_LEFT}" y="{MARGIN_TOP}" width="{plot_width}" height="{plot_height}" preserveAspectRatio="none"/>
"#,
            escape(&self.title),
            escape(png_href)
        );

        let (low_hz, high_hz) = self.frequency_range;
        let log_low = low_hz.max(1.0).log10();
        let log_high = high_hz.log10();
        for &hz in &[
            20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0,
        ] {
            if hz < low_hz || hz > high_hz {
                continue;
            }
            let fraction = (hz.log10() - log_low) / (log_high - log_low);
            let y = MARGIN_TOP + plot_height - fraction * plot_height;
            let _ = write!(
                svg,
                r#"<line x1="{:.1}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" stroke="white" stroke-width="0.5" opacity="0.25"/><text x="{:.1}" y="{:.1}" class="tick" text-anchor="end">{}</text>"#,
                MARGIN_LEFT,
                MARGIN_LEFT + plot_width,
                MARGIN_LEFT - 8.0,
                y + 4.0,
                if hz >= 1000.0 {
                    format!("{:.0}k", hz / 1000.0)
                } else {
                    format!("{hz:.0}")
                }
            );
        }

        let (start, end) = self.time_range;
        let span = (end - start).max(f64::MIN_POSITIVE);
        let step = nice_time_step(span);
        let mut time = (start / step).ceil() * step;
        while time <= end {
            let x = MARGIN_LEFT + (time - start) / span * plot_width;
            let _ = write!(
                svg,
                r#"<text x="{x:.1}" y="{:.1}" class="tick" text-anchor="middle">{}</text>"#,
                MARGIN_TOP + plot_height + 16.0,
                format_clock(time)
            );
            time += step;
        }

        let _ = write!(
            svg,
            r#"<rect x="{MARGIN_LEFT}" y="{MARGIN_TOP}" width="{plot_width}" height="{plot_height}" fill="none" class="axis"/>
<text x="{:.1}" y="{:.1}" class="label" text-anchor="middle">time</text>
<text transform="translate(16,{:.1}) rotate(-90)" class="label" text-anchor="middle">frequency (Hz)</text>
<text x="{:.1}" y="{:.1}" class="tick" text-anchor="end">{:.0} to {:.0} dB</text>
</svg>
"#,
            MARGIN_LEFT + plot_width / 2.0,
            height - 10.0,
            MARGIN_TOP + plot_height / 2.0,
            MARGIN_LEFT + plot_width,
            20.0,
            self.floor_db,
            self.ceiling_db
        );
        svg
    }
}

/// Dark-to-bright ramp with a monotonic lightness, so a louder bin always reads
/// as brighter and a printed or greyscaled report keeps its ordering.
fn colour(level: f32) -> [u8; 3] {
    const STOPS: [[f32; 3]; 6] = [
        [0.001, 0.000, 0.014],
        [0.232, 0.060, 0.437],
        [0.551, 0.161, 0.506],
        [0.867, 0.288, 0.409],
        [0.988, 0.646, 0.240],
        [0.987, 0.991, 0.750],
    ];
    let scaled = (level.clamp(0.0, 1.0) * (STOPS.len() - 1) as f32).min((STOPS.len() - 1) as f32);
    let lower = scaled.floor() as usize;
    let upper = (lower + 1).min(STOPS.len() - 1);
    let t = scaled - lower as f32;
    std::array::from_fn(|channel| {
        let value = STOPS[lower][channel] + (STOPS[upper][channel] - STOPS[lower][channel]) * t;
        (value.clamp(0.0, 1.0) * 255.0).round() as u8
    })
}

fn nice_time_step(span: f64) -> f64 {
    for step in [1.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0] {
        if span / step <= 12.0 {
            return step;
        }
    }
    1200.0
}

fn format_clock(seconds: f64) -> String {
    let total = seconds.round() as i64;
    format!("{}:{:02}", total / 60, total % 60)
}
