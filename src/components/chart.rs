//! Charts for report results, drawn as inline SVG.
//!
//! Deliberately not a JavaScript charting library: the result already arrives
//! as a `ReportTable` on both sides of the wire, so drawing it in the `view!`
//! macro keeps the chart server-rendered, hydration-safe, and free of a CDN
//! script the page would have to wait for. The three shapes below — bar, line,
//! pie — are what the reporting agent chooses between.

use leptos::prelude::*;

use crate::server_fns::reports::{ChartKind, ChartSpec, ReportTable};

/// Viewbox geometry. The SVG scales to its container, so these are relative.
const WIDTH: f64 = 900.0;
const HEIGHT: f64 = 380.0;
const PAD_LEFT: f64 = 72.0;
const PAD_RIGHT: f64 = 24.0;
const PAD_TOP: f64 = 24.0;
const PAD_BOTTOM: f64 = 64.0;

/// Most points a chart will draw. Beyond this the picture is noise and the
/// table is the better answer.
const MAX_POINTS: usize = 60;

/// The series palette, cycled when a report charts several columns at once.
const SERIES_COLORS: &[&str] = &[
    "#f472b6", "#38bdf8", "#a3e635", "#fbbf24", "#c084fc", "#2dd4bf", "#fb7185", "#94a3b8",
];

fn color(index: usize) -> &'static str {
    SERIES_COLORS[index % SERIES_COLORS.len()]
}

/// One resolved series: a column name and its values, row-ordered.
struct Series {
    name: String,
    values: Vec<f64>,
}

/// Pull the labels and series out of the table, honouring [`MAX_POINTS`].
fn resolve(table: &ReportTable, chart: &ChartSpec) -> (Vec<String>, Vec<Series>) {
    let rows = table.rows.len().min(MAX_POINTS);
    let label_index = table.column_index(&chart.label_column);

    let labels = (0..rows)
        .map(|row| match label_index {
            Some(index) => table.rows[row].get(index).cloned().unwrap_or_default(),
            None => (row + 1).to_string(),
        })
        .collect();

    let series = chart
        .value_columns
        .iter()
        .filter_map(|name| {
            let index = table.column_index(name)?;
            Some(Series {
                name: table.columns[index].name.clone(),
                values: (0..rows).map(|row| table.number_at(row, index)).collect(),
            })
        })
        .collect();

    (labels, series)
}

/// A number as read in a tooltip: exact to the cent, without a trailing `.0`.
fn fmt_value(value: f64) -> String {
    if value.fract().abs() < 1e-9 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.2}")
    }
}

/// A number as read on an axis, where space is short. Large values are
/// abbreviated and small ones keep at most one decimal.
fn fmt_axis(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude >= 1_000_000.0 {
        format!("{:.1}M", value / 1_000_000.0)
    } else if magnitude >= 1_000.0 {
        format!("{:.1}k", value / 1_000.0)
    } else if value.fract().abs() < 1e-9 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.1}")
    }
}

/// Shorten a label so the axis stays readable.
fn short_label(label: &str) -> String {
    if label.chars().count() <= 16 {
        return label.to_string();
    }
    let kept: String = label.chars().take(15).collect();
    format!("{kept}\u{2026}")
}

/// Draw a report result, or nothing when the result cannot be charted.
#[component]
pub fn ReportChart(table: ReportTable, chart: ChartSpec) -> impl IntoView {
    if !chart.is_drawable() || table.rows.is_empty() {
        return ().into_any();
    }

    let (labels, series) = resolve(&table, &chart);
    if series.is_empty() || labels.is_empty() {
        return ().into_any();
    }

    let truncated = table.rows.len() > MAX_POINTS;
    let legend = (chart.kind != ChartKind::Pie && series.len() > 1).then(|| {
        series
            .iter()
            .enumerate()
            .map(|(index, s)| {
                view! {
                    <span class="inline-flex items-center gap-1.5 text-xs text-slate-400">
                        <span
                            class="inline-block h-2.5 w-2.5 rounded-sm"
                            style=format!("background-color: {}", color(index))
                        ></span>
                        {s.name.clone()}
                    </span>
                }
            })
            .collect_view()
    });

    let body = match chart.kind {
        ChartKind::Pie => pie(&labels, &series[0]).into_any(),
        ChartKind::Line => cartesian(&labels, &series, true).into_any(),
        _ => cartesian(&labels, &series, false).into_any(),
    };

    view! {
        <div class="rounded-card border border-slate-800 bg-slate-900 shadow-card p-5">
            <div class="mb-3 flex flex-wrap items-center justify-between gap-3">
                <h2 class="text-sm font-semibold text-slate-200">{chart.kind.label()}</h2>
                <div class="flex flex-wrap items-center gap-3">{legend}</div>
            </div>
            {body}
            {truncated
                .then(|| {
                    view! {
                        <p class="mt-3 text-xs text-slate-500">
                            {format!(
                                "Charting the first {MAX_POINTS} of {} rows. The full result is in the table and the CSV.",
                                table.rows.len(),
                            )}
                        </p>
                    }
                })}
        </div>
    }
    .into_any()
}

/// A bar or line chart on a shared axis pair.
fn cartesian(labels: &[String], series: &[Series], line: bool) -> impl IntoView {
    let plot_width = WIDTH - PAD_LEFT - PAD_RIGHT;
    let plot_height = HEIGHT - PAD_TOP - PAD_BOTTOM;
    let count = labels.len().max(1);

    // Always include zero so bar lengths are honest.
    let max = series
        .iter()
        .flat_map(|s| s.values.iter().copied())
        .fold(0.0_f64, f64::max);
    let min = series
        .iter()
        .flat_map(|s| s.values.iter().copied())
        .fold(0.0_f64, f64::min);
    let span = if (max - min).abs() < f64::EPSILON {
        1.0
    } else {
        max - min
    };

    let y_of = move |value: f64| PAD_TOP + plot_height * (1.0 - (value - min) / span);
    let zero_y = y_of(0.0);

    // Five gridlines, labelled with the value they sit at.
    let gridlines = (0..=4)
        .map(|step| {
            let value = min + span * (step as f64) / 4.0;
            let y = y_of(value);
            view! {
                <g>
                    <line
                        x1=PAD_LEFT.to_string()
                        y1=y.to_string()
                        x2=(WIDTH - PAD_RIGHT).to_string()
                        y2=y.to_string()
                        stroke="#1e293b"
                        stroke-width="1"
                    />
                    <text
                        x=(PAD_LEFT - 10.0).to_string()
                        y=(y + 4.0).to_string()
                        text-anchor="end"
                        fill="#64748b"
                        font-size="12"
                    >
                        {fmt_axis(value)}
                    </text>
                </g>
            }
        })
        .collect_view();

    let slot = plot_width / count as f64;
    // Labels overlap once there are many; thin them rather than shrink them.
    let label_every = (count as f64 / 14.0).ceil().max(1.0) as usize;
    let axis_labels = labels
        .iter()
        .enumerate()
        .filter(|(index, _)| index % label_every == 0)
        .map(|(index, label)| {
            let x = PAD_LEFT + slot * (index as f64 + 0.5);
            view! {
                <text
                    x=x.to_string()
                    y=(HEIGHT - PAD_BOTTOM + 20.0).to_string()
                    text-anchor="end"
                    fill="#94a3b8"
                    font-size="12"
                    transform=format!("rotate(-35 {x} {})", HEIGHT - PAD_BOTTOM + 20.0)
                >
                    {short_label(label)}
                </text>
            }
        })
        .collect_view();

    let plot = if line {
        series
            .iter()
            .enumerate()
            .map(|(s_index, s)| {
                let points = s
                    .values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        format!(
                            "{},{}",
                            PAD_LEFT + slot * (index as f64 + 0.5),
                            y_of(*value)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let dots = s
                    .values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        view! {
                            <circle
                                cx=(PAD_LEFT + slot * (index as f64 + 0.5)).to_string()
                                cy=y_of(*value).to_string()
                                r="3"
                                fill=color(s_index)
                            >
                                <title>{format!("{}: {}", s.name, fmt_value(*value))}</title>
                            </circle>
                        }
                    })
                    .collect_view();
                view! {
                    <g>
                        <polyline
                            points=points
                            fill="none"
                            stroke=color(s_index)
                            stroke-width="2.5"
                            stroke-linejoin="round"
                            stroke-linecap="round"
                        />
                        {dots}
                    </g>
                }
            })
            .collect_view()
            .into_any()
    } else {
        let group_width = slot * 0.72;
        let bar_width = (group_width / series.len() as f64).max(1.0);
        series
            .iter()
            .enumerate()
            .map(|(s_index, s)| {
                let bars = s
                    .values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        let x = PAD_LEFT + slot * (index as f64 + 0.5) - group_width / 2.0
                            + bar_width * s_index as f64;
                        let y = y_of(*value);
                        let top = y.min(zero_y);
                        let height = (y - zero_y).abs().max(1.0);
                        view! {
                            <rect
                                x=x.to_string()
                                y=top.to_string()
                                width=bar_width.to_string()
                                height=height.to_string()
                                fill=color(s_index)
                                rx="2"
                            >
                                <title>
                                    {format!("{}: {}", labels[index], fmt_value(*value))}
                                </title>
                            </rect>
                        }
                    })
                    .collect_view();
                view! { <g>{bars}</g> }
            })
            .collect_view()
            .into_any()
    };

    view! {
        <svg
            viewBox=format!("0 0 {WIDTH} {HEIGHT}")
            class="w-full"
            role="img"
            preserveAspectRatio="xMidYMid meet"
        >
            {gridlines} {plot}
            <line
                x1=PAD_LEFT.to_string()
                y1=zero_y.to_string()
                x2=(WIDTH - PAD_RIGHT).to_string()
                y2=zero_y.to_string()
                stroke="#475569"
                stroke-width="1.5"
            />
            {axis_labels}
        </svg>
    }
}

/// A pie chart of one series. Non-positive slices are dropped: a pie can only
/// show parts of a whole.
fn pie(labels: &[String], series: &Series) -> impl IntoView {
    let slices: Vec<(String, f64)> = labels
        .iter()
        .cloned()
        .zip(series.values.iter().copied())
        .filter(|(_, value)| *value > 0.0)
        .collect();

    let total: f64 = slices.iter().map(|(_, value)| value).sum();
    if total <= 0.0 {
        return view! {
            <p class="p-6 text-sm text-slate-400">
                "This result has no positive values to divide into a pie."
            </p>
        }
        .into_any();
    }

    let (cx, cy, r) = (190.0_f64, 190.0_f64, 150.0_f64);
    let mut angle = -std::f64::consts::FRAC_PI_2;

    let mut paths = Vec::new();
    let mut legend = Vec::new();
    for (index, (label, value)) in slices.iter().enumerate() {
        let sweep = value / total * std::f64::consts::TAU;
        let end = angle + sweep;
        let (x1, y1) = (cx + r * angle.cos(), cy + r * angle.sin());
        let (x2, y2) = (cx + r * end.cos(), cy + r * end.sin());
        let large = if sweep > std::f64::consts::PI { 1 } else { 0 };

        // A single slice covering the whole circle has no arc to draw.
        let d = if slices.len() == 1 {
            format!(
                "M {cx} {} A {r} {r} 0 1 1 {} {} A {r} {r} 0 1 1 {cx} {} Z",
                cy - r,
                cx + r,
                cy,
                cy - r
            )
        } else {
            format!("M {cx} {cy} L {x1} {y1} A {r} {r} 0 {large} 1 {x2} {y2} Z")
        };

        let share = value / total * 100.0;
        paths.push(view! {
            <path d=d fill=color(index) stroke="#0f172a" stroke-width="2">
                <title>{format!("{label}: {} ({share:.1}%)", fmt_value(*value))}</title>
            </path>
        });
        legend.push(view! {
            <li class="flex items-center gap-2 text-sm">
                <span
                    class="inline-block h-3 w-3 shrink-0 rounded-sm"
                    style=format!("background-color: {}", color(index))
                ></span>
                <span class="truncate text-slate-300">{label.clone()}</span>
                <span class="ml-auto shrink-0 tabular-nums text-slate-400">
                    {format!("{} ({share:.1}%)", fmt_value(*value))}
                </span>
            </li>
        });

        angle = end;
    }

    view! {
        <div class="flex flex-col items-center gap-6 md:flex-row md:items-start">
            <svg viewBox="0 0 380 380" class="w-full max-w-xs shrink-0" role="img">
                {paths}
            </svg>
            <ul class="w-full space-y-1.5 md:max-w-md">{legend}</ul>
        </div>
    }
    .into_any()
}
