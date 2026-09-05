//! Hand-written SVG plots.
//!
//! SVG rather than a plotting crate: the plots here are one shape each, the axis
//! labels have to stay selectable text so a frequency can be copied out of a
//! report, and the alternative pulls a font stack and a raster pipeline into a
//! build that otherwise needs neither.

use std::fmt::Write as _;

const WIDTH: f64 = 960.0;
const HEIGHT: f64 = 340.0;
const MARGIN_LEFT: f64 = 68.0;
const MARGIN_RIGHT: f64 = 22.0;
const MARGIN_TOP: f64 = 34.0;
const MARGIN_BOTTOM: f64 = 46.0;

/// One line on a plot.
pub struct Series {
    pub label: String,
    pub colour: String,
    pub points: Vec<(f64, f64)>,
    pub fill: bool,
}

impl Series {
    pub fn new(
        label: impl Into<String>,
        colour: impl Into<String>,
        points: Vec<(f64, f64)>,
    ) -> Self {
        Series {
            label: label.into(),
            colour: colour.into(),
            points,
            fill: false,
        }
    }

    pub fn filled(mut self) -> Self {
        self.fill = true;
        self
    }

    /// Max-pool down to `limit` points.
    ///
    /// A novelty curve has tens of thousands of points and an SVG viewer draws
    /// every one of them. Max-pooling rather than sampling because the peaks are
    /// the signal: a decimated curve that drops them looks like a track with no
    /// onsets.
    pub fn max_pooled(mut self, limit: usize) -> Self {
        if self.points.len() <= limit || limit == 0 {
            return self;
        }
        let bucket = self.points.len().div_ceil(limit);
        self.points = self
            .points
            .chunks(bucket)
            .filter_map(|chunk| {
                chunk
                    .iter()
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|&(_, y)| (chunk[0].0, y))
            })
            .collect();
        self
    }
}

/// A vertical rule, for a beat or a candidate tempo.
pub struct Marker {
    pub x: f64,
    pub label: Option<String>,
    pub colour: String,
    pub dashed: bool,
}

pub struct LinePlot {
    pub title: String,
    pub x_label: String,
    pub y_label: String,
    pub series: Vec<Series>,
    pub markers: Vec<Marker>,
    pub log_x: bool,
    pub x_range: Option<(f64, f64)>,
    pub y_range: Option<(f64, f64)>,
}

impl LinePlot {
    pub fn new(
        title: impl Into<String>,
        x_label: impl Into<String>,
        y_label: impl Into<String>,
    ) -> Self {
        LinePlot {
            title: title.into(),
            x_label: x_label.into(),
            y_label: y_label.into(),
            series: Vec::new(),
            markers: Vec::new(),
            log_x: false,
            x_range: None,
            y_range: None,
        }
    }

    pub fn series(mut self, series: Series) -> Self {
        self.series.push(series);
        self
    }

    pub fn marker(mut self, marker: Marker) -> Self {
        self.markers.push(marker);
        self
    }

    pub fn log_x(mut self) -> Self {
        self.log_x = true;
        self
    }

    pub fn x_range(mut self, min: f64, max: f64) -> Self {
        self.x_range = Some((min, max));
        self
    }

    pub fn to_svg(&self) -> String {
        let (x_min, x_max) = self.x_range.unwrap_or_else(|| {
            bounds(
                self.series
                    .iter()
                    .flat_map(|s| s.points.iter().map(|p| p.0)),
            )
        });
        let (y_min, y_max) = self.y_range.unwrap_or_else(|| {
            let (low, high) = bounds(
                self.series
                    .iter()
                    .flat_map(|s| s.points.iter().map(|p| p.1)),
            );
            (low.min(0.0), high)
        });

        let plot_width = WIDTH - MARGIN_LEFT - MARGIN_RIGHT;
        let plot_height = HEIGHT - MARGIN_TOP - MARGIN_BOTTOM;
        let to_x = |value: f64| {
            let fraction = if self.log_x {
                (value.max(f64::MIN_POSITIVE).log10() - x_min.max(f64::MIN_POSITIVE).log10())
                    / (x_max.log10() - x_min.max(f64::MIN_POSITIVE).log10())
            } else {
                (value - x_min) / (x_max - x_min)
            };
            MARGIN_LEFT + fraction.clamp(0.0, 1.0) * plot_width
        };
        let to_y = |value: f64| {
            let fraction = (value - y_min) / (y_max - y_min).max(f64::MIN_POSITIVE);
            MARGIN_TOP + plot_height - fraction.clamp(0.0, 1.0) * plot_height
        };

        let mut svg = String::new();
        header(&mut svg, &self.title);

        for tick in axis_ticks(x_min, x_max, self.log_x) {
            let x = to_x(tick);
            let _ = write!(
                svg,
                r#"<line x1="{x:.1}" y1="{:.1}" x2="{x:.1}" y2="{:.1}" class="grid"/>"#,
                MARGIN_TOP,
                MARGIN_TOP + plot_height
            );
            let _ = write!(
                svg,
                r#"<text x="{x:.1}" y="{:.1}" class="tick" text-anchor="middle">{}</text>"#,
                MARGIN_TOP + plot_height + 16.0,
                format_tick(tick)
            );
        }
        for tick in axis_ticks(y_min, y_max, false) {
            let y = to_y(tick);
            let _ = write!(
                svg,
                r#"<line x1="{:.1}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" class="grid"/>"#,
                MARGIN_LEFT,
                MARGIN_LEFT + plot_width
            );
            let _ = write!(
                svg,
                r#"<text x="{:.1}" y="{:.1}" class="tick" text-anchor="end">{}</text>"#,
                MARGIN_LEFT - 8.0,
                y + 4.0,
                format_tick(tick)
            );
        }

        for marker in &self.markers {
            let x = to_x(marker.x);
            let dash = if marker.dashed {
                r#" stroke-dasharray="4 3""#
            } else {
                ""
            };
            let _ = write!(
                svg,
                r#"<line x1="{x:.1}" y1="{:.1}" x2="{x:.1}" y2="{:.1}" stroke="{}" stroke-width="1.3"{dash} opacity="0.95"/>"#,
                MARGIN_TOP,
                MARGIN_TOP + plot_height,
                marker.colour
            );
            if let Some(label) = &marker.label {
                // Below the legend when there is one, which occupies the same
                // corner and otherwise prints through these labels.
                let label_y = if self.series.len() > 1 {
                    MARGIN_TOP + 30.0
                } else {
                    MARGIN_TOP + 12.0
                };
                let _ = write!(
                    svg,
                    r#"<text x="{:.1}" y="{label_y:.1}" class="tick" fill="{}" text-anchor="start">{}</text>"#,
                    x + 3.0,
                    marker.colour,
                    escape(label)
                );
            }
        }

        for series in &self.series {
            let points: String = series
                .points
                .iter()
                .filter(|(x, y)| x.is_finite() && y.is_finite())
                .map(|&(x, y)| format!("{:.2},{:.2} ", to_x(x), to_y(y)))
                .collect();
            if series.fill {
                let _ = write!(
                    svg,
                    r#"<polygon points="{:.2},{:.2} {points}{:.2},{:.2}" fill="{}" opacity="0.28"/>"#,
                    to_x(series.points.first().map(|p| p.0).unwrap_or(x_min)),
                    to_y(y_min),
                    to_x(series.points.last().map(|p| p.0).unwrap_or(x_max)),
                    to_y(y_min),
                    series.colour
                );
            }
            let _ = write!(
                svg,
                r#"<polyline points="{points}" fill="none" stroke="{}" stroke-width="1.4"/>"#,
                series.colour
            );
        }

        // Legend, only when more than one line shares the axes.
        if self.series.len() > 1 {
            for (i, series) in self.series.iter().enumerate() {
                let x = MARGIN_LEFT + 8.0 + i as f64 * 190.0;
                let _ = write!(
                    svg,
                    r#"<rect x="{x:.1}" y="{:.1}" width="10" height="10" fill="{}"/><text x="{:.1}" y="{:.1}" class="tick">{}</text>"#,
                    MARGIN_TOP + 6.0,
                    series.colour,
                    x + 15.0,
                    MARGIN_TOP + 15.0,
                    escape(&series.label)
                );
            }
        }

        axes(
            &mut svg,
            plot_width,
            plot_height,
            &self.x_label,
            &self.y_label,
        );
        svg.push_str("</svg>\n");
        svg
    }
}

/// A categorical bar chart, for chroma and for ranked key correlations.
pub struct BarChart {
    pub title: String,
    pub y_label: String,
    pub bars: Vec<(String, f64)>,
    pub highlight: Option<usize>,
}

impl BarChart {
    pub fn new(
        title: impl Into<String>,
        y_label: impl Into<String>,
        bars: Vec<(String, f64)>,
    ) -> Self {
        BarChart {
            title: title.into(),
            y_label: y_label.into(),
            bars,
            highlight: None,
        }
    }

    pub fn highlight(mut self, index: usize) -> Self {
        self.highlight = Some(index);
        self
    }

    pub fn to_svg(&self) -> String {
        let plot_width = WIDTH - MARGIN_LEFT - MARGIN_RIGHT;
        let plot_height = HEIGHT - MARGIN_TOP - MARGIN_BOTTOM;
        let (low, high) = bounds(self.bars.iter().map(|b| b.1));
        let y_min = low.min(0.0);
        let y_max = high.max(0.0);
        let to_y = |value: f64| {
            MARGIN_TOP + plot_height
                - ((value - y_min) / (y_max - y_min).max(f64::MIN_POSITIVE)) * plot_height
        };

        let mut svg = String::new();
        header(&mut svg, &self.title);
        for tick in axis_ticks(y_min, y_max, false) {
            let y = to_y(tick);
            let _ = write!(
                svg,
                r#"<line x1="{:.1}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" class="grid"/><text x="{:.1}" y="{:.1}" class="tick" text-anchor="end">{}</text>"#,
                MARGIN_LEFT,
                MARGIN_LEFT + plot_width,
                MARGIN_LEFT - 8.0,
                y + 4.0,
                format_tick(tick)
            );
        }

        let slot = plot_width / self.bars.len().max(1) as f64;
        for (i, (label, value)) in self.bars.iter().enumerate() {
            let x = MARGIN_LEFT + i as f64 * slot;
            let top = to_y(value.max(0.0));
            let bottom = to_y(value.min(0.0));
            let colour = if self.highlight == Some(i) {
                "#c2410c"
            } else {
                "#2563eb"
            };
            let _ = write!(
                svg,
                r#"<rect x="{:.1}" y="{top:.1}" width="{:.1}" height="{:.1}" fill="{colour}" opacity="0.85"/>"#,
                x + slot * 0.12,
                slot * 0.76,
                (bottom - top).max(0.5)
            );
            let _ = write!(
                svg,
                r#"<text x="{:.1}" y="{:.1}" class="tick" text-anchor="middle">{}</text>"#,
                x + slot / 2.0,
                MARGIN_TOP + plot_height + 16.0,
                escape(label)
            );
        }

        axes(&mut svg, plot_width, plot_height, "", &self.y_label);
        svg.push_str("</svg>\n");
        svg
    }
}

/// Open an SVG, with its stylesheet scoped to the figure it belongs to.
///
/// Every selector is prefixed with the root's own class. A `<style>` inside
/// inline SVG is scoped to the document, not to the SVG, so the unprefixed
/// `.label` and `.title` this used to emit restyled whatever the surrounding
/// page called by those names. The browser interface calls its captions
/// `.label`, and opening a figure there resized every one of them on the page.
/// The prefix costs nothing in a standalone file, where the root carries the
/// class and the descendant selectors still match.
fn header(svg: &mut String, title: &str) {
    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" class="dubplate-figure" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}" font-family="ui-sans-serif, system-ui, sans-serif">
<style>.dubplate-figure .grid{{stroke:#e2e8f0;stroke-width:1}}.dubplate-figure .tick{{font-size:11px;fill:#475569}}.dubplate-figure .axis{{stroke:#94a3b8;stroke-width:1}}.dubplate-figure .title{{font-size:14px;fill:#0f172a;font-weight:600}}.dubplate-figure .label{{font-size:12px;fill:#334155}}</style>
<rect width="{WIDTH}" height="{HEIGHT}" fill="white"/>
<text x="{MARGIN_LEFT}" y="20" class="title">{}</text>
"#,
        escape(title)
    );
}

fn axes(svg: &mut String, plot_width: f64, plot_height: f64, x_label: &str, y_label: &str) {
    let _ = write!(
        svg,
        r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" class="axis"/><line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" class="axis"/>"#,
        MARGIN_LEFT,
        MARGIN_TOP + plot_height,
        MARGIN_LEFT + plot_width,
        MARGIN_TOP + plot_height,
        MARGIN_LEFT,
        MARGIN_TOP,
        MARGIN_LEFT,
        MARGIN_TOP + plot_height
    );
    if !x_label.is_empty() {
        let _ = write!(
            svg,
            r#"<text x="{:.1}" y="{:.1}" class="label" text-anchor="middle">{}</text>"#,
            MARGIN_LEFT + plot_width / 2.0,
            HEIGHT - 10.0,
            escape(x_label)
        );
    }
    if !y_label.is_empty() {
        let _ = write!(
            svg,
            r#"<text transform="translate(16,{:.1}) rotate(-90)" class="label" text-anchor="middle">{}</text>"#,
            MARGIN_TOP + plot_height / 2.0,
            escape(y_label)
        );
    }
}

fn bounds(values: impl Iterator<Item = f64>) -> (f64, f64) {
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for v in values.filter(|v| v.is_finite()) {
        low = low.min(v);
        high = high.max(v);
    }
    if !low.is_finite() || !high.is_finite() || (high - low).abs() < f64::EPSILON {
        (0.0, 1.0)
    } else {
        (low, high)
    }
}

/// Round tick positions: decades on a log axis, a 1/2/5 step on a linear one.
fn axis_ticks(min: f64, max: f64, logarithmic: bool) -> Vec<f64> {
    if logarithmic {
        let mut ticks = Vec::new();
        let mut decade = 10f64.powf(min.max(1e-6).log10().floor());
        while decade <= max {
            for multiple in [1.0, 2.0, 5.0] {
                let tick = decade * multiple;
                if tick >= min && tick <= max {
                    ticks.push(tick);
                }
            }
            decade *= 10.0;
        }
        return ticks;
    }

    let span = (max - min).abs().max(f64::MIN_POSITIVE);
    let rough = span / 6.0;
    let magnitude = 10f64.powf(rough.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * magnitude)
        .find(|s| *s >= rough)
        .unwrap_or(magnitude);
    let first = (min / step).ceil() * step;
    let mut ticks = Vec::new();
    let mut tick = first;
    while tick <= max + step * 0.001 {
        ticks.push(tick);
        tick += step;
    }
    ticks
}

fn format_tick(value: f64) -> String {
    match value.abs() {
        magnitude if magnitude >= 10.0 => format!("{value:.0}"),
        magnitude if magnitude >= 1.0 => format!("{value:.1}"),
        _ => format!("{value:.2}"),
    }
}

pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod scoping {
    /// A figure's stylesheet may not name anything the page around it owns.
    ///
    /// These SVGs are inlined into a page, and a `<style>` inside inline SVG
    /// applies to the whole document. `.label` and `.title` are names any page
    /// might use for its own text, so every rule here has to be reachable only
    /// through the figure's root.
    #[test]
    fn a_figure_styles_nothing_outside_itself() {
        let mut svg = String::new();
        super::header(&mut svg, "a title");
        let style = svg
            .split_once("<style>")
            .and_then(|(_, rest)| rest.split_once("</style>"))
            .map(|(style, _)| style)
            .expect("the header writes a stylesheet");

        for rule in style.split('}').filter(|rule| !rule.trim().is_empty()) {
            let selector = rule.split('{').next().unwrap_or_default().trim();
            assert!(
                selector.starts_with(".dubplate-figure "),
                "`{selector}` escapes the figure and restyles the page around it",
            );
        }
        assert!(
            svg.contains(r#"class="dubplate-figure""#),
            "the root carries the class the rules hang off"
        );
    }
}
