//! The spreadsheet import workspace: read a file, agree on what each column
//! means, then import it as one watched task.
//!
//! Laid out as the same numbered steps as the bulk-property and contact-mail
//! pages, and sharing the latter's single-task machinery, so "start it and watch
//! it" behaves the same way in both tools.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::use_query_map;

use crate::components::guard::{require_information_management_access, require_operations_admin};
use crate::components::layout::Layout;
use crate::server_fns::contacts::ContactType;
use crate::server_fns::crm_import::{
    cancel_import_task, load_import_task, plan_import, start_import_task, ColumnPlan, ColumnTarget,
    CoreField, ImportPlan, ImportPolicy, ImportPreview, ImportTask, ImportTaskStatus, MatchAction,
    MatchKind, PropertySuggestion,
};
use crate::server_fns::err_text;
use crate::server_fns::property_filters::PropertySubject;
use crate::state::AppState;

const INPUT: &str = "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40 disabled:cursor-not-allowed disabled:opacity-50";
const LABEL: &str = "mb-1 block text-xs font-medium text-slate-400";
const PANEL: &str = "rounded-card border border-slate-800 bg-slate-900 shadow-card p-5";

/// The `<select>` value meaning "a property, named by the inputs beside it".
const PROPERTY_OPTION: &str = "__property";
const IGNORE_OPTION: &str = "__ignore";

#[component]
pub fn CrmImportPage() -> impl IntoView {
    let state = expect_context::<AppState>();
    require_operations_admin(state, move || {
        require_information_management_access(state, move || {
            view! {
                <Layout title="Import from a file".to_string()>
                    <CrmImportWorkspace />
                </Layout>
            }
            .into_any()
        })
    })
}

/// Browser `confirm`, or a refusal during SSR where no dialog exists.
fn confirm(message: &str) -> bool {
    #[cfg(feature = "hydrate")]
    {
        web_sys::window()
            .and_then(|window| window.confirm_with_message(message).ok())
            .unwrap_or(false)
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = message;
        false
    }
}

fn plural(count: i64, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

#[component]
fn CrmImportWorkspace() -> impl IntoView {
    let query_map = use_query_map();
    let initial_subject = query_map
        .with_untracked(|map| map.get("subject").map(|value| value.to_string()))
        .and_then(|value| PropertySubject::from_slug(&value))
        .unwrap_or_default();

    let subject = RwSignal::new(initial_subject);
    let preview = RwSignal::new(None::<ImportPreview>);
    let columns = RwSignal::new(Vec::<ColumnPlan>::new());
    let policy = RwSignal::new(ImportPolicy::default());
    let plan = RwSignal::new(None::<ImportPlan>);
    let task = RwSignal::new(None::<ImportTask>);

    let uploading = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let task_poll_generation = RwSignal::new(0u64);

    let active = Signal::derive(move || task.get().is_some_and(|task| task.status.is_active()));
    let has_file = Signal::derive(move || preview.get().is_some());
    let review_count = Signal::derive(move || {
        columns
            .get()
            .iter()
            .filter(|column| column.match_kind.needs_review() && !column.target.is_ignored())
            .count() as i64
    });
    let known_properties =
        Signal::derive(move || preview.get().map(|p| p.known_properties).unwrap_or_default());
    // Hoisted: a `>` inside the `view!` macro is parsed as a tag close.
    let has_review_work = Signal::derive(move || review_count.get() > 0);

    // The one active import is polled the same way the mail task is, so the
    // panel keeps moving without the user touching anything.
    let refresh_task = move || {
        task_poll_generation.update(|generation| *generation += 1);
        let generation = task_poll_generation.get_untracked();
        spawn_local(async move {
            if let Ok(latest) = load_import_task().await {
                if task_poll_generation.get_untracked() == generation {
                    task.set(latest);
                }
            }
        });
    };
    Effect::new(move |_| {
        refresh_task();
        if let Ok(handle) = set_interval_with_handle(refresh_task, std::time::Duration::from_secs(2))
        {
            on_cleanup(move || handle.clear());
        }
    });

    let reset_file = move || {
        preview.set(None);
        columns.set(Vec::new());
        plan.set(None);
        error.set(String::new());
        notice.set(String::new());
    };

    let switch_subject = move |next: PropertySubject| {
        if subject.get_untracked() == next {
            return;
        }
        subject.set(next);
        // The mapping was classified against the other subject's vocabulary, so
        // none of it carries over.
        reset_file();
    };

    // Reading the chosen file has to happen in the browser, so this is the one
    // place the page talks to `web_sys` directly.
    let upload = move |_| {
        #[cfg(feature = "hydrate")]
        {
            use wasm_bindgen::JsCast;

            let Some(input) = web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| document.get_element_by_id("import-file"))
                .and_then(|element| element.dyn_into::<web_sys::HtmlInputElement>().ok())
            else {
                return;
            };
            let Some(file) = input.files().and_then(|files| files.get(0)) else {
                error.set("Choose a file first.".to_string());
                return;
            };

            let form_data = match web_sys::FormData::new() {
                Ok(form_data) => form_data,
                Err(_) => {
                    error.set("That file could not be read.".to_string());
                    return;
                }
            };
            let _ = form_data.append_with_blob_and_filename("file", &file, &file.name());
            let _ = form_data.append_with_str("subject", subject.get_untracked().slug());

            uploading.set(true);
            error.set(String::new());
            notice.set(String::new());
            plan.set(None);
            spawn_local(async move {
                match crate::server_fns::crm_import::upload_import_file(form_data.into()).await {
                    Ok(result) => {
                        columns.set(result.columns.clone());
                        notice.set(format!(
                            "Read {} from {}.",
                            plural(result.row_count, "row", "rows"),
                            result.file_name
                        ));
                        preview.set(Some(result));
                    }
                    Err(server_error) => {
                        preview.set(None);
                        columns.set(Vec::new());
                        error.set(err_text(server_error));
                    }
                }
                uploading.set(false);
            });
        }
    };

    let run_plan = move || {
        let Some(current) = preview.get_untracked() else {
            return;
        };
        let chosen = subject.get_untracked();
        let mapping = columns.get_untracked();
        let chosen_policy = policy.get_untracked();
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match plan_import(current.upload_id, chosen, mapping, chosen_policy).await {
                Ok(result) => {
                    plan.set(Some(result));
                    error.set(String::new());
                }
                Err(server_error) => {
                    plan.set(None);
                    error.set(err_text(server_error));
                }
            }
            busy.set(false);
        });
    };

    let launch = move |_| {
        if busy.get_untracked() || active.get_untracked() {
            return;
        }
        let Some(current) = preview.get_untracked() else {
            return;
        };
        let Some(current_plan) = plan.get_untracked() else {
            error.set("Check the import first, so you can see what it will do.".to_string());
            return;
        };
        if !current_plan.writes_anything() {
            error.set("This import would not create or update anything.".to_string());
            return;
        }

        let chosen = subject.get_untracked();
        let noun = chosen.noun_plural();
        let mut message = format!(
            "Import {} from \"{}\"?\n\n  Create {} new {noun}\n  Update {} existing {noun}",
            plural(current_plan.total_rows, "row", "rows"),
            current.file_name,
            current_plan.will_create,
            current_plan.will_update,
        );
        if current_plan.will_skip > 0 {
            message.push_str(&format!("\n  Skip {}", current_plan.will_skip));
        }
        if current_plan.invalid > 0 {
            message.push_str(&format!(
                "\n  {} row(s) cannot be imported and will be reported",
                current_plan.invalid
            ));
        }
        if !current_plan.new_properties.is_empty() {
            message.push_str(&format!(
                "\n\nThis creates {} that {noun} do not have yet:\n{}",
                plural(current_plan.new_properties.len() as i64, "property", "properties"),
                current_plan
                    .new_properties
                    .iter()
                    .map(|name| format!("  - {name}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if current_plan.new_organizations > 0 {
            message.push_str(&format!(
                "\n\nIt also creates {}.",
                plural(
                    current_plan.new_organizations,
                    "new organization",
                    "new organizations"
                )
            ));
        }
        if !confirm(&message) {
            return;
        }

        let mapping = columns.get_untracked();
        let chosen_policy = policy.get_untracked();
        task_poll_generation.update(|generation| *generation += 1);
        busy.set(true);
        error.set(String::new());
        notice.set(String::new());
        spawn_local(async move {
            match start_import_task(current.upload_id, chosen, mapping, chosen_policy).await {
                Ok(started) => {
                    task.set(Some(started));
                    notice.set(
                        "Import started. This page updates as it runs.".to_string(),
                    );
                    // The staged upload is consumed by starting; a second run
                    // needs the file again.
                    preview.set(None);
                    columns.set(Vec::new());
                    plan.set(None);
                }
                Err(server_error) => error.set(err_text(server_error)),
            }
            busy.set(false);
        });
    };

    let cancel_task = Callback::new(move |()| {
        let Some(current) = task.get_untracked() else {
            return;
        };
        if !confirm(
            "Stop this import? Rows already imported are kept; the rest are left untouched.",
        ) {
            return;
        }
        task_poll_generation.update(|generation| *generation += 1);
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match cancel_import_task(current.id).await {
                Ok(updated) => {
                    let cancelling = updated.status == ImportTaskStatus::Cancelling;
                    task.set(Some(updated));
                    notice.set(if cancelling {
                        "Stopping after the current row.".to_string()
                    } else {
                        "The import had already finished.".to_string()
                    });
                }
                Err(server_error) => error.set(err_text(server_error)),
            }
            busy.set(false);
        });
    });

    let subject_button = move |target: PropertySubject| {
        view! {
            <button
                type="button"
                on:click=move |_| switch_subject(target)
                class=move || {
                    if subject.get() == target {
                        "rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white"
                    } else {
                        "rounded-full border border-slate-700 px-4 py-2 text-sm font-medium text-slate-300 hover:bg-slate-800"
                    }
                }
            >
                {target.label()}
            </button>
        }
    };

    view! {
        <div class="space-y-5">
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div>
                    <p class="max-w-3xl text-sm text-slate-400">
                        "Read a spreadsheet, check what every column will set, then import it in one go."
                    </p>
                    <p class="mt-1 text-xs text-slate-500">
                        "Nothing is written until you confirm. Blank cells are left alone rather than clearing what a record already holds."
                    </p>
                </div>
                <A
                    href=move || {
                        match subject.get() {
                            PropertySubject::Contact => "/contacts".to_string(),
                            PropertySubject::Organization => "/organizations".to_string(),
                        }
                    }
                    attr:class="rounded-full border border-slate-700 px-3 py-2 text-sm text-slate-300 hover:bg-slate-800"
                >
                    "Back"
                </A>
            </div>

            <Show when=move || !error.get().is_empty()>
                <p role="alert" class="rounded-lg border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-sm text-rose-300">
                    {move || error.get()}
                </p>
            </Show>
            <Show when=move || !notice.get().is_empty()>
                <p role="status" class="rounded-lg border border-emerald-500/40 bg-emerald-500/10 px-3 py-2 text-sm text-emerald-300">
                    {move || notice.get()}
                </p>
            </Show>

            <ImportTaskProgress task cancel=cancel_task busy />

            // One import at a time: while it runs, the whole form is inert. The
            // server refuses a second start regardless.
            <fieldset prop:disabled=move || active.get() class="min-w-0 space-y-5 disabled:opacity-60">
                <section class=PANEL>
                    <h2 class="text-lg font-semibold text-slate-100">"1. Choose the file"</h2>
                    <div class="mt-3 flex flex-wrap gap-2">
                        {PropertySubject::ALL.iter().map(|target| subject_button(*target)).collect_view()}
                    </div>
                    <p class="mt-2 text-xs text-slate-500">
                        {move || match subject.get() {
                            PropertySubject::Contact => "Each row is one person. Rows are matched to existing people by email address.",
                            PropertySubject::Organization => "Each row is one organization. Rows are matched to existing organizations by name.",
                        }}
                    </p>

                    <div class="mt-4 grid gap-3 sm:grid-cols-[1fr_auto] sm:items-end">
                        <label>
                            <span class=LABEL>"Spreadsheet"</span>
                            <input
                                id="import-file"
                                type="file"
                                accept=".csv,.tsv,.xlsx"
                                class=INPUT
                                on:change=move |_| { reset_file(); }
                            />
                        </label>
                        <button
                            type="button"
                            on:click=upload
                            prop:disabled=move || uploading.get()
                            class="rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                        >
                            {move || if uploading.get() { "Reading\u{2026}" } else { "Read file" }}
                        </button>
                    </div>
                    <p class="mt-2 text-xs text-slate-500">
                        ".csv, .tsv, or .xlsx, up to 10 MB and 5,000 rows. The first row must hold the column names. Older .xls files need re-saving as .xlsx first."
                    </p>
                </section>

                <Show when=move || has_file.get()>
                    <FilePreview preview />
                    <MappingReview
                        subject
                        columns
                        known=known_properties
                        review_count
                        on_change=Callback::new(move |()| plan.set(None))
                    />
                    <PolicyPanel subject policy on_change=Callback::new(move |()| plan.set(None)) />

                    <section class=PANEL>
                        <h2 class="text-lg font-semibold text-slate-100">"5. Check and import"</h2>
                        <p class="mt-1 text-sm text-slate-500">
                            "See exactly what the import would do before committing it."
                        </p>

                        <Show when=move || has_review_work.get()>
                            <p class="mt-3 rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm text-amber-200">
                                {move || plural(review_count.get(), "column", "columns")}
                                {move || if review_count.get() == 1 { " still needs" } else { " still need" }}
                                " a decision in step 3. You can import anyway, but a new property will be created for each."
                            </p>
                        </Show>

                        <Show when=move || plan.get().is_some()>
                            {move || {
                                let Some(result) = plan.get() else { return ().into_any() };
                                let new_properties = result.new_properties.clone();
                                let warnings = result.warnings.clone();
                                let new_organizations = result.new_organizations;
                                view! {
                                    <dl class="mt-4 grid gap-3 sm:grid-cols-4">
                                        <PlanTile label="Will be created" value=result.will_create tone="text-emerald-300" />
                                        <PlanTile label="Will be updated" value=result.will_update tone="text-primary-300" />
                                        <PlanTile label="Will be skipped" value=result.will_skip tone="text-slate-100" />
                                        <PlanTile label="Cannot be imported" value=result.invalid tone="text-rose-300" />
                                    </dl>
                                    {(!new_properties.is_empty()).then(|| view! {
                                        <div class="mt-3 rounded-lg border border-slate-800 bg-slate-950 p-3">
                                            <p class="text-xs text-slate-500">"New properties this creates"</p>
                                            <ul class="mt-1 flex flex-wrap gap-1.5">
                                                {new_properties.into_iter().map(|name| view! {
                                                    <li class="rounded-full bg-primary-500/15 px-2 py-0.5 text-xs text-primary-300">{name}</li>
                                                }).collect_view()}
                                            </ul>
                                        </div>
                                    })}
                                    {(new_organizations > 0).then(|| view! {
                                        <p class="mt-3 text-sm text-slate-400">
                                            "It also creates " {plural(new_organizations, "organization", "organizations")}
                                            " named in the file that do not exist yet."
                                        </p>
                                    })}
                                    {(!warnings.is_empty()).then(|| view! {
                                        <details class="mt-3 rounded-lg border border-amber-500/20 bg-amber-500/5">
                                            <summary class="cursor-pointer px-3 py-2 text-sm font-medium text-amber-300">
                                                "Rows that cannot be imported"
                                            </summary>
                                            <div class="border-t border-amber-500/20 px-3 py-2">
                                                {warnings.into_iter().map(|warning| view! {
                                                    <p class="text-xs text-amber-200">{warning}</p>
                                                }).collect_view()}
                                            </div>
                                        </details>
                                    })}
                                }
                                .into_any()
                            }}
                        </Show>

                        <div class="mt-4 flex flex-wrap gap-2">
                            <button
                                type="button"
                                on:click=move |_| run_plan()
                                prop:disabled=move || busy.get()
                                class="rounded-full border border-slate-700 px-4 py-2 text-sm font-medium text-slate-200 hover:bg-slate-800 disabled:opacity-50"
                            >
                                {move || if busy.get() { "Checking\u{2026}" } else { "Check the import" }}
                            </button>
                            <button
                                type="button"
                                on:click=launch
                                prop:disabled=move || busy.get() || plan.get().is_none()
                                class="rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:opacity-50"
                            >
                                {move || if busy.get() { "Starting\u{2026}" } else { "Import" }}
                            </button>
                        </div>
                    </section>
                </Show>
            </fieldset>
        </div>
    }
}

#[component]
fn PlanTile(label: &'static str, value: i64, tone: &'static str) -> impl IntoView {
    view! {
        <div class="rounded-lg border border-slate-800 bg-slate-950 p-3">
            <dt class="text-xs text-slate-500">{label}</dt>
            <dd class=format!("mt-1 text-lg font-semibold {tone}")>{value}</dd>
        </div>
    }
}

/// Step 2: the file as it was actually read, so a mis-parsed file is obvious
/// before anyone thinks about mapping it.
#[component]
fn FilePreview(preview: RwSignal<Option<ImportPreview>>) -> impl IntoView {
    move || {
        let Some(current) = preview.get() else {
            return ().into_any();
        };
        let headers = current.headers.clone();
        let rows = current.sample_rows.clone();
        let shown = rows.len();
        view! {
            <section class=PANEL>
                <div class="flex flex-wrap items-start justify-between gap-3">
                    <div>
                        <h2 class="text-lg font-semibold text-slate-100">"2. Check the file"</h2>
                        <p class="mt-1 text-sm text-slate-500">
                            "The first " {shown} " rows, exactly as they were read."
                        </p>
                    </div>
                    <div class="text-right text-xs text-slate-500">
                        <p class="text-sm font-medium text-slate-300">{current.file_name.clone()}</p>
                        {(!current.sheet_name.is_empty()).then(|| view! {
                            <p>"Sheet: " {current.sheet_name.clone()}</p>
                        })}
                        <p>{current.row_count} " rows, " {headers.len()} " columns"</p>
                    </div>
                </div>

                <div class="mt-4 w-full max-w-full overflow-x-auto rounded-lg border border-slate-800 bg-slate-950">
                    <table class="w-full min-w-max text-left text-xs">
                        <thead>
                            <tr class="border-b border-slate-800">
                                {headers.iter().map(|header| view! {
                                    <th class="whitespace-nowrap px-3 py-2 font-semibold text-slate-300">
                                        {header.clone()}
                                    </th>
                                }).collect_view()}
                            </tr>
                        </thead>
                        <tbody>
                            {rows.into_iter().map(|row| view! {
                                <tr class="border-b border-slate-800/60 last:border-b-0">
                                    {row.into_iter().map(|cell| view! {
                                        <td class="max-w-xs truncate px-3 py-2 text-slate-400">{cell}</td>
                                    }).collect_view()}
                                </tr>
                            }).collect_view()}
                        </tbody>
                    </table>
                </div>
            </section>
        }
        .into_any()
    }
}

/// Step 3: what every column will set, with the ambiguous ones pinned first.
///
/// This is the panel the whole page exists for. A column that merely *resembles*
/// an existing property is never adopted silently: it is raised here with the
/// candidates and their usage counts, and the user picks.
#[component]
fn MappingReview(
    subject: RwSignal<PropertySubject>,
    columns: RwSignal<Vec<ColumnPlan>>,
    known: Signal<Vec<PropertySuggestion>>,
    review_count: Signal<i64>,
    on_change: Callback<()>,
) -> impl IntoView {
    // Flagged columns first, then file order, so the questions are at the top
    // without losing the correspondence to the spreadsheet.
    let ordered = Signal::derive(move || {
        let mut all: Vec<ColumnPlan> = columns.get();
        all.sort_by_key(|column| {
            (
                !(column.match_kind.needs_review() && !column.target.is_ignored()),
                column.index,
            )
        });
        all
    });

    view! {
        <section class=PANEL>
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div>
                    <h2 class="text-lg font-semibold text-slate-100">"3. Review what each column sets"</h2>
                    <p class="mt-1 max-w-3xl text-sm text-slate-500">
                        "Each column is matched against the fields and properties "
                        {move || subject.get().noun_plural()}
                        " already use. Anything ambiguous is flagged rather than guessed at."
                    </p>
                </div>
                <span class=move || {
                    if review_count.get() > 0 {
                        "rounded-full bg-amber-500/15 px-3 py-1 text-sm font-semibold text-amber-300"
                    } else {
                        "rounded-full bg-emerald-500/15 px-3 py-1 text-sm font-semibold text-emerald-300"
                    }
                }>
                    {move || {
                        let count = review_count.get();
                        if count == 0 {
                            "Nothing needs review".to_string()
                        } else {
                            format!(
                                "{} {} review",
                                plural(count, "column", "columns"),
                                if count == 1 { "needs" } else { "need" }
                            )
                        }
                    }}
                </span>
            </div>

            <div class="mt-4 space-y-3">
                {move || ordered.get().into_iter().map(|column| view! {
                    <ColumnCard column subject columns known on_change />
                }).collect_view()}
            </div>

            // Shared by every card's section and name inputs, so the vocabulary
            // already in use is one keystroke away from any column.
            <datalist id="import-sections">
                {move || {
                    let mut sections: Vec<String> = known
                        .get()
                        .into_iter()
                        .map(|option| option.section)
                        .filter(|section| !section.is_empty())
                        .collect();
                    sections.sort();
                    sections.dedup();
                    sections.into_iter().map(|section| view! {
                        <option value=section></option>
                    }).collect_view()
                }}
            </datalist>
            <datalist id="import-property-keys">
                {move || known.get().into_iter().map(|option| {
                    let label = option.label();
                    view! { <option value=option.key>{label}</option> }
                }).collect_view()}
            </datalist>
        </section>
    }
}

#[component]
fn ColumnCard(
    column: ColumnPlan,
    subject: RwSignal<PropertySubject>,
    columns: RwSignal<Vec<ColumnPlan>>,
    known: Signal<Vec<PropertySuggestion>>,
    on_change: Callback<()>,
) -> impl IntoView {
    let index = column.index;
    let header = column.header.clone();
    let samples = column.sample_values.clone();
    let has_samples = !samples.is_empty();
    let sample_text = samples.join(" \u{00b7} ");
    let suggestions = column.suggestions.clone();
    let has_suggestions = !suggestions.is_empty();
    let flagged = column.match_kind.needs_review() && !column.target.is_ignored();

    // Every mutation goes through here so the note and the badge can never
    // disagree with the target they describe.
    let retarget = move |target: ColumnTarget, match_kind: Option<MatchKind>| {
        let vocabulary = known.get_untracked();
        let chosen = subject.get_untracked();
        columns.update(|all| {
            if let Some(entry) = all.iter_mut().find(|entry| entry.index == index) {
                entry.note =
                    crate::server_fns::crm_import::describe_target(chosen, &target, &vocabulary);
                if let Some(kind) = match_kind {
                    entry.match_kind = kind;
                }
                entry.target = target;
            }
        });
        on_change.run(());
    };

    // The card reads its own row back out of the list, so adopting a suggestion
    // updates the select, the inputs, and the sentence together.
    let current = Signal::derive(move || {
        columns
            .get()
            .into_iter()
            .find(|entry| entry.index == index)
            .unwrap_or_default()
    });

    let select_value = Signal::derive(move || match &current.get().target {
        ColumnTarget::Ignore => IGNORE_OPTION.to_string(),
        ColumnTarget::Core { field } => field.slug().to_string(),
        ColumnTarget::Property { .. } => PROPERTY_OPTION.to_string(),
    });
    let is_property = Signal::derive(move || current.get().target.property().is_some());

    view! {
        <div class=move || {
            let base = "rounded-lg border p-4";
            if flagged && current.get().match_kind.needs_review() {
                format!("{base} border-amber-500/40 bg-amber-500/5")
            } else {
                format!("{base} border-slate-800 bg-slate-950")
            }
        }>
            <div class="flex flex-wrap items-start justify-between gap-2">
                <div class="min-w-0">
                    <p class="text-sm font-semibold text-slate-100">{header.clone()}</p>
                    <Show when=move || has_samples>
                        <p class="mt-0.5 truncate text-xs text-slate-500">
                            "e.g. " {sample_text.clone()}
                        </p>
                    </Show>
                </div>
                <span class=move || format!(
                    "shrink-0 rounded-full px-2.5 py-1 text-xs font-semibold {}",
                    current.get().match_kind.badge_classes()
                )>
                    {move || current.get().match_kind.label()}
                </span>
            </div>

            <div class="mt-3 grid gap-3 md:grid-cols-3">
                <label class=move || if is_property.get() { "md:col-span-1" } else { "md:col-span-3" }>
                    <span class=LABEL>"Imports into"</span>
                    <select
                        class=INPUT
                        prop:value=move || select_value.get()
                        on:change=move |event| {
                            let value = event_target_value(&event);
                            if value == IGNORE_OPTION {
                                retarget(ColumnTarget::Ignore, Some(MatchKind::Ignored));
                            } else if value == PROPERTY_OPTION {
                                retarget(
                                    ColumnTarget::Property {
                                        section: String::new(),
                                        key: header.clone(),
                                    },
                                    Some(MatchKind::NewProperty),
                                );
                            } else if let Some(field) = CoreField::from_slug(&value) {
                                retarget(ColumnTarget::Core { field }, Some(MatchKind::CoreField));
                            }
                        }
                    >
                        <option value=IGNORE_OPTION>"Skip this column"</option>
                        <optgroup label="Built-in fields">
                            {move || CoreField::for_subject(subject.get()).iter().map(|field| view! {
                                <option value=field.slug()>{field.label()}</option>
                            }).collect_view()}
                        </optgroup>
                        <optgroup label="Property">
                            <option value=PROPERTY_OPTION>"A property (named below)"</option>
                        </optgroup>
                    </select>
                </label>

                <Show when=move || is_property.get()>
                    <label>
                        <span class=LABEL>"Section"</span>
                        <input
                            class=INPUT
                            list="import-sections"
                            placeholder="General"
                            prop:value=move || current.get().target.property().map(|(section, _)| section).unwrap_or_default()
                            on:input=move |event| {
                                let section = event_target_value(&event);
                                if let Some((_, key)) = current.get_untracked().target.property() {
                                    retarget(ColumnTarget::Property { section, key }, None);
                                }
                            }
                        />
                    </label>
                    <label>
                        <span class=LABEL>"Property name"</span>
                        <input
                            class=INPUT
                            list="import-property-keys"
                            prop:value=move || current.get().target.property().map(|(_, key)| key).unwrap_or_default()
                            on:input=move |event| {
                                let key = event_target_value(&event);
                                if let Some((section, _)) = current.get_untracked().target.property() {
                                    retarget(ColumnTarget::Property { section, key }, None);
                                }
                            }
                        />
                    </label>
                </Show>
            </div>

            <p class="mt-2 text-xs text-slate-400">{move || current.get().note}</p>

            <Show when=move || has_suggestions && current.get().match_kind.needs_review()>
                <div class="mt-3 border-t border-amber-500/20 pt-3">
                    <p class="text-xs font-medium text-amber-200">
                        {move || {
                            if current.get().match_kind == MatchKind::SectionConflict {
                                "A property of this name already exists under another section:"
                            } else {
                                "This looks like an existing property. Did you mean one of these?"
                            }
                        }}
                    </p>
                    <div class="mt-2 flex flex-wrap gap-2">
                        {suggestions.clone().into_iter().map(|suggestion| {
                            let label = suggestion.label();
                            let reason = suggestion.reason.clone();
                            let title = suggestion.reason.clone();
                            let section = suggestion.section.clone();
                            let key = suggestion.key.clone();
                            view! {
                                <button
                                    type="button"
                                    title=title
                                    on:click=move |_| retarget(
                                        ColumnTarget::Property {
                                            section: section.clone(),
                                            key: key.clone(),
                                        },
                                        Some(MatchKind::ExistingProperty),
                                    )
                                    class="rounded-xl border border-amber-500/40 bg-amber-500/10 px-3 py-1.5 text-left text-xs text-amber-100 hover:bg-amber-500/20"
                                >
                                    <span class="block font-semibold">"Use " {label}</span>
                                    <span class="block text-amber-300/80">{reason}</span>
                                </button>
                            }
                        }).collect_view()}
                        <button
                            type="button"
                            on:click=move |_| {
                                if let Some((section, key)) = current.get_untracked().target.property() {
                                    retarget(
                                        ColumnTarget::Property { section, key },
                                        Some(MatchKind::NewProperty),
                                    );
                                }
                            }
                            class="rounded-full border border-slate-700 px-3 py-1.5 text-xs text-slate-300 hover:bg-slate-800"
                        >
                            "Keep as a new property"
                        </button>
                    </div>
                </div>
            </Show>
        </div>
    }
}

/// Step 4: what happens when a row already exists.
#[component]
fn PolicyPanel(
    subject: RwSignal<PropertySubject>,
    policy: RwSignal<ImportPolicy>,
    on_change: Callback<()>,
) -> impl IntoView {
    let is_people = Signal::derive(move || subject.get() == PropertySubject::Contact);

    view! {
        <section class=PANEL>
            <h2 class="text-lg font-semibold text-slate-100">"4. Handle rows that already exist"</h2>
            <p class="mt-1 text-sm text-slate-500">
                {move || match subject.get() {
                    PropertySubject::Contact => "A row matches an existing person when the email address is the same, ignoring case.",
                    PropertySubject::Organization => "A row matches an existing organization when the name is the same, ignoring case and spacing.",
                }}
            </p>

            <div class="mt-4 grid gap-3 md:grid-cols-2">
                <label>
                    <span class=LABEL>"When the record already exists"</span>
                    <select
                        class=INPUT
                        prop:value=move || policy.get().on_match.slug()
                        on:change=move |event| {
                            if let Some(action) = MatchAction::from_slug(&event_target_value(&event)) {
                                policy.update(|policy| policy.on_match = action);
                                on_change.run(());
                            }
                        }
                    >
                        {MatchAction::ALL.iter().map(|action| view! {
                            <option value=action.slug()>{action.label()}</option>
                        }).collect_view()}
                    </select>
                </label>
            </div>

            <label class="mt-3 flex cursor-pointer items-start gap-2 text-sm text-slate-300">
                <input
                    type="checkbox"
                    class="mt-0.5 h-4 w-4 accent-primary-500"
                    prop:checked=move || policy.get().create_missing
                    on:change=move |event| {
                        let checked = event_target_checked(&event);
                        policy.update(|policy| policy.create_missing = checked);
                        on_change.run(());
                    }
                />
                <span>
                    "Create records that do not exist yet"
                    <span class="block text-xs text-slate-500">
                        "Turn this off to only fill in records you already have."
                    </span>
                </span>
            </label>

            <Show when=move || is_people.get()>
                <label class="mt-3 block max-w-md">
                    <span class=LABEL>"File new people as"</span>
                    <select
                        class=INPUT
                        prop:value=move || policy.get().default_contact_type.slug()
                        on:change=move |event| {
                            if let Some(kind) = ContactType::from_slug(&event_target_value(&event)) {
                                policy.update(|policy| policy.default_contact_type = kind);
                                on_change.run(());
                            }
                        }
                    >
                        {ContactType::ALL.iter().map(|kind| view! {
                            <option value=kind.slug()>{kind.label()}</option>
                        }).collect_view()}
                    </select>
                    <span class="mt-1 block text-xs text-slate-500">
                        "Every person needs at least one type. A column mapped to Contact types wins per row; this covers the rest. Existing people keep the types they have."
                    </span>
                </label>

                <label class="mt-3 flex cursor-pointer items-start gap-2 text-sm text-slate-300">
                    <input
                        type="checkbox"
                        class="mt-0.5 h-4 w-4 accent-primary-500"
                        prop:checked=move || policy.get().link_organization_by_name
                        on:change=move |event| {
                            let checked = event_target_checked(&event);
                            policy.update(|policy| policy.link_organization_by_name = checked);
                            on_change.run(());
                        }
                    />
                    <span>
                        "File people under the organization named in the file"
                        <span class="block text-xs text-slate-500">
                            "Needs a column mapped to Organization. Matched by name."
                        </span>
                    </span>
                </label>
                <label class="mt-3 flex cursor-pointer items-start gap-2 text-sm text-slate-300">
                    <input
                        type="checkbox"
                        class="mt-0.5 h-4 w-4 accent-primary-500"
                        prop:checked=move || policy.get().create_missing_organizations
                        on:change=move |event| {
                            let checked = event_target_checked(&event);
                            policy.update(|policy| policy.create_missing_organizations = checked);
                            on_change.run(());
                        }
                    />
                    <span>
                        "Create organizations that do not exist yet"
                        <span class="block text-xs text-slate-500">
                            "Off by default: a typo in one row would otherwise become a permanent organization."
                        </span>
                    </span>
                </label>
            </Show>
        </section>
    }
}

/// The one import task, polled while it runs.
#[component]
fn ImportTaskProgress(
    task: RwSignal<Option<ImportTask>>,
    cancel: Callback<()>,
    busy: RwSignal<bool>,
) -> impl IntoView {
    move || {
        let Some(current) = task.get() else {
            return ().into_any();
        };
        let status = current.status;
        let processed = current.processed_count();
        let percent = current.percent();
        let status_class = match status {
            ImportTaskStatus::Completed => "bg-emerald-500/15 text-emerald-300",
            ImportTaskStatus::Cancelled => "bg-slate-700 text-slate-200",
            ImportTaskStatus::Failed => "bg-rose-500/15 text-rose-300",
            ImportTaskStatus::Cancelling => "bg-amber-500/15 text-amber-300",
            _ => "bg-primary-500/15 text-primary-300",
        };
        let failures = current.failures.clone();
        let completed_at = current.completed_at.clone();
        let cancel_requested_at = current.cancel_requested_at.clone();
        let cancel_requested_by = current.cancel_requested_by_name.clone();
        let task_error = current.error.clone();

        view! {
            <section class=PANEL>
                <div class="flex flex-wrap items-start justify-between gap-3">
                    <div>
                        <div class="flex flex-wrap items-center gap-2">
                            <h2 class="text-lg font-semibold text-slate-100">"Import"</h2>
                            <span class=format!("rounded-full px-2.5 py-1 text-xs font-semibold {status_class}")>
                                {status.label()}
                            </span>
                        </div>
                        <p class="mt-1 text-sm font-medium text-slate-300">
                            {current.file_name.clone()}
                            " \u{2192} " {current.subject.noun_plural()}
                        </p>
                        <p class="mt-1 text-xs text-slate-500">
                            "Started by " {current.created_by_name.clone()} " on " {current.created_at.clone()}
                        </p>
                    </div>
                    <Show when=move || status.is_active()>
                        <button
                            type="button"
                            on:click=move |_| cancel.run(())
                            prop:disabled=move || busy.get() || status == ImportTaskStatus::Cancelling
                            class="rounded-full border border-rose-500/50 px-3 py-2 text-sm font-medium text-rose-300 hover:bg-rose-500/10 disabled:opacity-50"
                        >
                            {if status == ImportTaskStatus::Cancelling { "Stopping\u{2026}" } else { "Stop import" }}
                        </button>
                    </Show>
                </div>

                <div class="mt-5" role="progressbar" aria-label="Import progress"
                    aria-valuemin="0" aria-valuemax=current.row_total aria-valuenow=processed>
                    <div class="h-3 overflow-hidden rounded-full bg-slate-800">
                        <div class="h-full rounded-full bg-primary-500 transition-all" style=format!("width: {percent}%")></div>
                    </div>
                    <p class="mt-2 text-sm text-slate-300">
                        {processed} " of " {current.row_total} " rows processed (" {percent} "%)"
                    </p>
                </div>

                <dl class="mt-4 grid gap-3 text-sm sm:grid-cols-4">
                    <div class="rounded-lg bg-slate-950 p-3">
                        <dt class="text-xs text-slate-500">"Created"</dt>
                        <dd class="mt-1 text-lg font-semibold text-emerald-300">{current.created_count}</dd>
                    </div>
                    <div class="rounded-lg bg-slate-950 p-3">
                        <dt class="text-xs text-slate-500">"Updated"</dt>
                        <dd class="mt-1 text-lg font-semibold text-primary-300">{current.updated_count}</dd>
                    </div>
                    <div class="rounded-lg bg-slate-950 p-3">
                        <dt class="text-xs text-slate-500">"Skipped"</dt>
                        <dd class="mt-1 text-lg font-semibold text-slate-100">{current.skipped_count}</dd>
                    </div>
                    <div class="rounded-lg bg-slate-950 p-3">
                        <dt class="text-xs text-slate-500">"Failed"</dt>
                        <dd class="mt-1 text-lg font-semibold text-rose-300">{current.failed_count}</dd>
                    </div>
                </dl>

                {(!completed_at.is_empty()).then(|| view! {
                    <p class="mt-3 text-xs text-slate-500">"Finished: " {completed_at}</p>
                })}
                {(!cancel_requested_at.is_empty()).then(|| view! {
                    <p class="mt-2 text-xs text-amber-300">
                        "Stop requested by " {cancel_requested_by} " on " {cancel_requested_at}
                    </p>
                })}
                {(!task_error.is_empty()).then(|| view! {
                    <p class="mt-3 text-sm text-rose-300" role="alert">{task_error}</p>
                })}
                {(!failures.is_empty()).then(|| {
                    let count = failures.len();
                    view! {
                        <details class="mt-4 rounded-lg border border-rose-500/20 bg-rose-500/5">
                            <summary class="cursor-pointer px-3 py-2 text-sm font-medium text-rose-300">
                                "Rows that could not be imported (" {count} ")"
                            </summary>
                            <div class="border-t border-rose-500/20">
                                {failures.into_iter().map(|failure| view! {
                                    <div class="border-b border-rose-500/10 px-3 py-2 text-xs last:border-b-0">
                                        <p class="text-slate-200">
                                            "Row " {failure.row}
                                            {(!failure.label.is_empty()).then(|| format!(" \u{00b7} {}", failure.label))}
                                        </p>
                                        <p class="mt-1 text-rose-300">{failure.error}</p>
                                    </div>
                                }).collect_view()}
                            </div>
                        </details>
                    }
                })}
            </section>
        }
        .into_any()
    }
}
