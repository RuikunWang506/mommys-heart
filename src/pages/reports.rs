//! The reports workspace: a library of saved reports, plus the AI page that
//! builds new ones.
//!
//! `/reports` lists what has been kept; `/reports/new` is the one-input
//! generator; `/reports/:id` re-runs a saved report from its stored query,
//! which needs no model at all.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map};

use crate::components::chart::ReportChart;
use crate::components::guard::require_operations_admin;
use crate::components::layout::Layout;
use crate::components::loading::Loading;
use crate::server_fns::err_text;
use crate::server_fns::reports::{
    build_report, delete_saved_report, list_saved_reports, load_saved_report, run_saved_report,
    save_report, Report, SavedReport, MAX_REQUEST_CHARS, MAX_TITLE_CHARS,
};
use crate::state::AppState;

const PANEL: &str = "rounded-card border border-slate-800 bg-slate-900 shadow-card p-5";
const INPUT: &str =
    "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-600 focus:border-primary-500 focus:outline-none";

/// Starting points, so the first use of the page is not a blank box.
const EXAMPLES: &[&str] = &[
    "Funding received by month this year, as a line chart",
    "How many cases are in each status",
    "Top 10 organizations by total funding, excluding voided records",
    "Volunteer hours logged per volunteer over the last 6 months",
];

/// The saved-report library at `/reports`.
#[component]
pub fn ReportsPage() -> impl IntoView {
    let state = expect_context::<AppState>();
    require_operations_admin(state, move || {
        view! {
            <Layout title="Reports".to_string()>
                <SavedReportList />
            </Layout>
        }
        .into_any()
    })
}

/// The AI generator at `/reports/new`.
#[component]
pub fn NewReportPage() -> impl IntoView {
    let state = expect_context::<AppState>();
    require_operations_admin(state, move || {
        view! {
            <Layout title="New report".to_string()>
                <ReportsWorkspace />
            </Layout>
        }
        .into_any()
    })
}

/// One saved report at `/reports/:id`.
#[component]
pub fn SavedReportPage() -> impl IntoView {
    let state = expect_context::<AppState>();
    let params = use_params_map();
    require_operations_admin(state, move || {
        let id = params.read().get("id").unwrap_or_default();
        view! {
            <Layout title="Report".to_string()>
                <SavedReportDetail report_id=id />
            </Layout>
        }
        .into_any()
    })
}

#[component]
fn SavedReportList() -> impl IntoView {
    let keyword = RwSignal::new(String::new());
    let items = RwSignal::new(Vec::<SavedReport>::new());
    let total = RwSignal::new(0i64);
    let window = RwSignal::new(25i64);
    let loading = RwSignal::new(true);
    let error = RwSignal::new(String::new());

    Effect::new(move |_| {
        let search = keyword.get();
        let limit = window.get();
        loading.set(true);
        spawn_local(async move {
            match list_saved_reports(search, 0, limit).await {
                Ok(page) => {
                    items.set(page.items);
                    total.set(page.total);
                    error.set(String::new());
                }
                Err(e) => error.set(err_text(e)),
            }
            loading.set(false);
        });
    });

    let rows = move || {
        let reports = items.get();
        if reports.is_empty() {
            return (!loading.get())
                .then(|| {
                    view! {
                        <p class="rounded-lg border border-slate-800 bg-slate-950 px-4 py-6 text-sm text-slate-500">
                            "No saved reports yet. Build one and save it to keep it here."
                        </p>
                    }
                })
                .into_any();
        }
        reports
            .into_iter()
            .map(|report| {
                let href = format!("/reports/{}", report.id);
                let last_run = if report.last_run_at.is_empty() {
                    "not run since it was saved".to_string()
                } else {
                    format!("last run {}", report.last_run_at)
                };
                let author = if report.author_name.is_empty() {
                    "Unknown author".to_string()
                } else {
                    report.author_name.clone()
                };
                let meta = format!(
                    "{author} \u{00b7} saved {} \u{00b7} {last_run}",
                    report.created_at,
                );
                let summary = report.summary.clone();
                view! {
                    <A
                        href=href
                        attr:class="block rounded-lg border border-slate-800 bg-slate-950 p-4 transition-colors hover:border-primary-500/40 hover:bg-slate-900"
                    >
                        <p class="font-medium text-slate-100">{report.title.clone()}</p>
                        {(!summary.is_empty())
                            .then(|| view! { <p class="mt-1 text-sm text-slate-400">{summary}</p> })}
                        <p class="mt-2 text-xs text-slate-500">{meta}</p>
                    </A>
                }
            })
            .collect_view()
            .into_any()
    };

    view! {
        <div class="space-y-6">
            <section class=PANEL>
                <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                    <div>
                        <h2 class="text-lg font-semibold text-slate-100">"Saved reports"</h2>
                        <p class="mt-1 text-sm text-slate-500">
                            "Open one to run it again. A saved report re-uses its query, so it answers straight away and never calls the assistant."
                        </p>
                    </div>
                    <A
                        href="/reports/new"
                        attr:class="shrink-0 rounded-full bg-primary-500 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-primary-400"
                    >
                        "New report"
                    </A>
                </div>

                <input
                    class=format!("{INPUT} mt-4 max-w-md")
                    placeholder="Search by title, request or author"
                    prop:value=move || keyword.get()
                    on:input=move |event| {
                        window.set(25);
                        keyword.set(event_target_value(&event));
                    }
                />

                {move || {
                    let message = error.get();
                    (!message.is_empty())
                        .then(|| {
                            view! {
                                <p class="mt-4 rounded-lg border border-rose-500/40 bg-rose-500/10 px-4 py-3 text-sm text-rose-200">
                                    {message}
                                </p>
                            }
                        })
                }}

                <Show when=move || loading.get()>
                    <div class="mt-4">
                        <Loading />
                    </div>
                </Show>

                <div class="mt-4 space-y-2">{rows}</div>

                <div class="mt-4 flex items-center justify-between text-xs text-slate-500">
                    <span>
                        "Showing " {move || items.get().len()} " of " {move || total.get()}
                        " reports"
                    </span>
                    <Show when=move || (items.get().len() as i64) < total.get()>
                        <button
                            type="button"
                            on:click=move |_| window.update(|value| *value += 25)
                            class="rounded-full border border-slate-700 px-3 py-1.5 text-sm text-slate-200 hover:bg-slate-800"
                        >
                            "Load more reports"
                        </button>
                    </Show>
                </div>
            </section>
        </div>
    }
}

#[component]
fn SavedReportDetail(report_id: String) -> impl IntoView {
    let navigate = use_navigate();
    let saved = RwSignal::new(None::<SavedReport>);
    let report = RwSignal::new(None::<Report>);
    let error = RwSignal::new(String::new());
    let busy = RwSignal::new(true);
    let show_working = RwSignal::new(false);
    let id = StoredValue::new(report_id);

    let run = move || {
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            let this = id.get_value();
            if let Ok(Some(details)) = load_saved_report(this.clone()).await {
                saved.set(Some(details));
            }
            match run_saved_report(this).await {
                Ok(result) => report.set(Some(result)),
                Err(server_error) => error.set(err_text(server_error)),
            }
            busy.set(false);
        });
    };

    Effect::new(move |previous: Option<()>| {
        if previous.is_none() {
            run();
        }
    });

    let remove = move |_| {
        let navigate = navigate.clone();
        spawn_local(async move {
            match delete_saved_report(id.get_value()).await {
                Ok(()) => navigate("/reports", Default::default()),
                Err(server_error) => error.set(err_text(server_error)),
            }
        });
    };

    view! {
        <div class="space-y-6">
            <section class=PANEL>
                <div class="flex flex-wrap items-start justify-between gap-3">
                    <div class="min-w-0">
                        <A href="/reports" attr:class="text-xs text-slate-400 hover:text-slate-200">
                            "\u{2190} All reports"
                        </A>
                        {move || {
                            saved
                                .get()
                                .map(|details| {
                                    let author = if details.author_name.is_empty() {
                                        "Unknown author".to_string()
                                    } else {
                                        details.author_name.clone()
                                    };
                                    let meta = format!(
                                        "Saved by {author} on {}",
                                        details.created_at,
                                    );
                                    let request = details.request.clone();
                                    view! {
                                        <p class="mt-2 text-xs text-slate-500">{meta}</p>
                                        {(!request.is_empty())
                                            .then(|| {
                                                view! {
                                                    <p class="mt-1 text-xs text-slate-500">
                                                        "Originally asked: " {request}
                                                    </p>
                                                }
                                            })}
                                    }
                                })
                        }}
                    </div>
                    <div class="flex shrink-0 gap-2">
                        <button
                            on:click=move |_| run()
                            disabled=move || busy.get()
                            class="rounded-full bg-primary-500 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-primary-400 disabled:cursor-not-allowed disabled:opacity-60"
                        >
                            {move || if busy.get() { "Running\u{2026}" } else { "Run again" }}
                        </button>
                        <button
                            on:click=remove
                            class="rounded-full border border-rose-500/40 px-4 py-2 text-sm font-medium text-rose-300 transition-colors hover:bg-rose-500/10"
                        >
                            "Delete"
                        </button>
                    </div>
                </div>
            </section>

            {move || {
                let message = error.get();
                (!message.is_empty())
                    .then(|| {
                        view! {
                            <p class="rounded-lg border border-rose-500/40 bg-rose-500/10 px-4 py-3 text-sm text-rose-200">
                                {message}
                            </p>
                        }
                    })
            }}

            <Show when=move || busy.get()>
                <div class=PANEL>
                    <Loading />
                </div>
            </Show>

            {move || {
                report
                    .get()
                    .map(|result| view! { <ReportView report=result show_working /> })
            }}
        </div>
    }
}

#[component]
fn ReportsWorkspace() -> impl IntoView {
    let request = RwSignal::new(String::new());
    let report = RwSignal::new(None::<Report>);
    let error = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let show_working = RwSignal::new(false);

    let run = move || {
        let text = request.get_untracked().trim().to_string();
        if text.is_empty() {
            error.set("Describe the report you want.".to_string());
            return;
        }
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(String::new());
        report.set(None);
        show_working.set(false);
        spawn_local(async move {
            match build_report(text).await {
                Ok(result) => report.set(Some(result)),
                Err(server_error) => error.set(err_text(server_error)),
            }
            busy.set(false);
        });
    };

    // Ctrl/Cmd+Enter submits, since the request box is a textarea.
    let on_keydown = move |event: leptos::ev::KeyboardEvent| {
        if event.key() == "Enter" && (event.ctrl_key() || event.meta_key()) {
            event.prevent_default();
            run();
        }
    };

    view! {
        <div class="space-y-6">
            <section class=PANEL>
                <label for="report-request" class="mb-1 block text-sm font-medium text-slate-200">
                    "What do you want to know?"
                </label>
                <p class="mb-3 text-xs text-slate-400">
                    "Describe the report in plain language. An assistant reads the database structure, runs read-only queries to check the data, then builds the chart and table. It can never change or delete anything."
                </p>
                <textarea
                    id="report-request"
                    rows="3"
                    maxlength=MAX_REQUEST_CHARS.to_string()
                    placeholder="e.g. Total funding received per month this year, as a line chart"
                    class="w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40 disabled:cursor-not-allowed disabled:opacity-50"
                    prop:value=move || request.get()
                    on:input=move |event| request.set(event_target_value(&event))
                    on:keydown=on_keydown
                    disabled=move || busy.get()
                ></textarea>

                <div class="mt-3 flex flex-wrap items-center gap-3">
                    <button
                        on:click=move |_| run()
                        disabled=move || busy.get()
                        class="rounded-full bg-primary-500 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-primary-400 disabled:cursor-not-allowed disabled:opacity-60"
                    >
                        {move || if busy.get() { "Building\u{2026}" } else { "Build report" }}
                    </button>
                    <span class="text-xs text-slate-500">"Ctrl+Enter to run"</span>
                </div>

                <div class="mt-4 flex flex-wrap gap-2">
                    {EXAMPLES
                        .iter()
                        .map(|example| {
                            view! {
                                <button
                                    on:click=move |_| request.set(example.to_string())
                                    disabled=move || busy.get()
                                    class="rounded-full border border-slate-700 px-3 py-1 text-xs text-slate-300 transition-colors hover:border-primary-500/40 hover:bg-slate-800 disabled:opacity-50"
                                >
                                    {*example}
                                </button>
                            }
                        })
                        .collect_view()}
                </div>
            </section>

            {move || {
                let message = error.get();
                (!message.is_empty())
                    .then(|| {
                        view! {
                            <p class="rounded-lg border border-rose-500/40 bg-rose-500/10 px-4 py-3 text-sm text-rose-200">
                                {message}
                            </p>
                        }
                    })
            }}

            {move || {
                busy.get()
                    .then(|| {
                        view! {
                            <div class=PANEL>
                                <div class="flex items-center gap-3 text-sm text-slate-400">
                                    <span class="h-4 w-4 animate-spin rounded-full border-2 border-slate-700 border-t-primary-500"></span>
                                    "Reading the database structure and checking the data\u{2026} this usually takes under a minute."
                                </div>
                            </div>
                        }
                    })
            }}

            {move || {
                report
                    .get()
                    .map(|result| {
                        let prose = request.get_untracked();
                        view! {
                            <SaveReportPanel report=result.clone() request=prose />
                            <ReportView report=result show_working />
                        }
                    })
            }}
        </div>
    }
}

/// Keeping a built report. Only a title is asked for; the query, the chart and
/// the author come from what is already on the page.
#[component]
fn SaveReportPanel(report: Report, request: String) -> impl IntoView {
    let navigate = use_navigate();
    let title = RwSignal::new(report.title.clone());
    let error = RwSignal::new(String::new());
    let saving = RwSignal::new(false);
    let details = StoredValue::new((report, request));

    let save = move |_| {
        if saving.get_untracked() {
            return;
        }
        let name = title.get_untracked().trim().to_string();
        if name.is_empty() {
            error.set("Give the report a title.".to_string());
            return;
        }
        saving.set(true);
        error.set(String::new());
        let navigate = navigate.clone();
        spawn_local(async move {
            let (report, request) = details.get_value();
            match save_report(name, request, report.summary, report.sql, report.chart).await {
                Ok(id) => navigate(&format!("/reports/{id}"), Default::default()),
                Err(server_error) => {
                    error.set(err_text(server_error));
                    saving.set(false);
                }
            }
        });
    };

    view! {
        <section class=PANEL>
            <p class="text-sm font-medium text-slate-200">"Save this report"</p>
            <p class="mt-1 text-xs text-slate-400">
                "Saving keeps the query, so anyone can run it again later without the assistant."
            </p>
            <div class="mt-3 flex flex-wrap items-center gap-3">
                <input
                    class=format!("{INPUT} max-w-md")
                    maxlength=MAX_TITLE_CHARS.to_string()
                    placeholder="Report title"
                    prop:value=move || title.get()
                    on:input=move |event| title.set(event_target_value(&event))
                />
                <button
                    on:click=save
                    disabled=move || saving.get()
                    class="rounded-full bg-primary-500 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-primary-400 disabled:cursor-not-allowed disabled:opacity-60"
                >
                    {move || if saving.get() { "Saving\u{2026}" } else { "Save report" }}
                </button>
            </div>
            {move || {
                let message = error.get();
                (!message.is_empty())
                    .then(|| view! { <p class="mt-2 text-xs text-rose-300">{message}</p> })
            }}
        </section>
    }
}

#[component]
fn ReportView(report: Report, show_working: RwSignal<bool>) -> impl IntoView {
    let table = report.table.clone();
    let download_name = format!("{}.csv", slugify(&report.title));
    let download_href = csv_data_url(&report.csv);
    let row_label = format!(
        "{} row{}{}",
        table.rows.len(),
        if table.rows.len() == 1 { "" } else { "s" },
        if table.truncated {
            format!(
                " (capped at {})",
                crate::server_fns::reports::MAX_REPORT_ROWS
            )
        } else {
            String::new()
        },
    );
    let step_count = report.steps.len();
    let summary = report.summary.clone();

    view! {
        <section class="space-y-6">
            <div class=PANEL>
                <div class="flex flex-wrap items-start justify-between gap-4">
                    <div class="min-w-0">
                        <h2 class="text-lg font-semibold tracking-tight text-slate-100">
                            {report.title.clone()}
                        </h2>
                        {(!summary.is_empty())
                            .then(|| {
                                view! { <p class="mt-1 text-sm text-slate-400">{summary}</p> }
                            })}
                        <p class="mt-2 text-xs text-slate-500">{row_label}</p>
                    </div>
                    <a
                        href=download_href
                        download=download_name
                        class="shrink-0 rounded-full border border-primary-500/40 bg-primary-500/10 px-4 py-2 text-sm font-medium text-primary-300 transition-colors hover:bg-primary-500/20"
                    >
                        "Export to CSV"
                    </a>
                </div>
            </div>

            <ReportChart table=report.table.clone() chart=report.chart.clone() />

            <ResultTable table=table.clone() />

            <div class=PANEL>
                <button
                    on:click=move |_| show_working.update(|open| *open = !*open)
                    class="text-sm font-medium text-slate-300 hover:text-slate-100"
                >
                    {move || {
                        if show_working.get() {
                            "Hide how this was built".to_string()
                        } else {
                            format!(
                                "Show how this was built ({step_count} exploratory quer{})",
                                if step_count == 1 { "y" } else { "ies" },
                            )
                        }
                    }}
                </button>

                <div class:hidden=move || !show_working.get() class="mt-4 space-y-4">
                    {report
                        .steps
                        .iter()
                        .map(|step| {
                            let outcome = if step.error.is_empty() {
                                format!("{} row(s)", step.row_count)
                            } else {
                                step.error.clone()
                            };
                            let outcome_class = if step.error.is_empty() {
                                "mt-1 text-xs text-slate-500"
                            } else {
                                "mt-1 text-xs text-amber-300"
                            };
                            view! {
                                <div class="rounded-lg border border-slate-800 bg-slate-950 p-3">
                                    <p class="text-xs font-medium text-slate-300">
                                        {step.note.clone()}
                                    </p>
                                    <pre class="mt-2 overflow-x-auto text-xs leading-relaxed text-slate-400">
                                        {step.sql.clone()}
                                    </pre>
                                    <p class=outcome_class>{outcome}</p>
                                </div>
                            }
                        })
                        .collect_view()}

                    <div>
                        <p class="text-xs font-medium text-slate-300">"Final query"</p>
                        <pre class="mt-2 overflow-x-auto rounded-lg border border-slate-800 bg-slate-950 p-3 text-xs leading-relaxed text-slate-300">
                            {report.sql.clone()}
                        </pre>
                    </div>
                </div>
            </div>
        </section>
    }
}

#[component]
fn ResultTable(table: crate::server_fns::reports::ReportTable) -> impl IntoView {
    if table.columns.is_empty() {
        return ().into_any();
    }
    if table.rows.is_empty() {
        return view! {
            <div class=PANEL>
                <p class="text-sm text-slate-400">"That query matched no rows."</p>
            </div>
        }
        .into_any();
    }

    let numeric: Vec<bool> = table.columns.iter().map(|c| c.numeric).collect();

    view! {
        <div class="overflow-x-auto rounded-card border border-slate-800 bg-slate-900 shadow-card">
            <table class="min-w-full text-sm">
                <thead class="border-b border-slate-800 bg-slate-900/60">
                    <tr>
                        {table
                            .columns
                            .iter()
                            .map(|column| {
                                let align = if column.numeric { "text-right" } else { "text-left" };
                                view! {
                                    <th class=format!(
                                        "whitespace-nowrap px-4 py-2.5 font-medium text-slate-300 {align}",
                                    )>{column.name.clone()}</th>
                                }
                            })
                            .collect_view()}
                    </tr>
                </thead>
                <tbody>
                    {table
                        .rows
                        .iter()
                        .map(|row| {
                            let numeric = numeric.clone();
                            view! {
                                <tr class="border-b border-slate-800/60 last:border-0 hover:bg-slate-800/40">
                                    {row
                                        .iter()
                                        .enumerate()
                                        .map(|(index, cell)| {
                                            let align = if numeric.get(index).copied().unwrap_or(false) {
                                                "text-right tabular-nums"
                                            } else {
                                                "text-left"
                                            };
                                            view! {
                                                <td class=format!(
                                                    "px-4 py-2 text-slate-300 {align}",
                                                )>{cell.clone()}</td>
                                            }
                                        })
                                        .collect_view()}
                                </tr>
                            }
                        })
                        .collect_view()}
                </tbody>
            </table>
        </div>
    }
    .into_any()
}

/// A `data:` URL the browser saves straight to disk.
///
/// The CSV is already in hand, so there is nothing to fetch: an ordinary
/// `<a download>` is enough and the export needs no extra endpoint. The
/// leading BOM is what makes Excel read UTF-8 accents correctly.
fn csv_data_url(csv: &str) -> String {
    let mut encoded = String::with_capacity(csv.len() * 3);
    for byte in format!("\u{feff}{csv}").bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    format!("data:text/csv;charset=utf-8,{encoded}")
}

/// A filename-safe version of the report title.
fn slugify(title: &str) -> String {
    let slug: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "report".to_string()
    } else {
        slug.chars().take(60).collect()
    }
}
