//! The audit change-log viewer.
//!
//! The audit log is its own data source (see [`crate::server_fns::audit`]): it is
//! never bundled into the `User` or `Case` payloads. This component fetches it
//! independently, on demand, when a user opens the change log for an entity. It
//! shows its own loading state, paginates ("Load more"), and lets the user
//! restrict the history to a start/end date range — so no view ever pulls a whole
//! audit history into the browser at once.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::components::loading::Loading;
use crate::server_fns::audit::{list_audit_page, AuditScope, ChangeLogEntry};
use crate::server_fns::err_text;
use crate::state::{today, AppState};

/// How many rows load per page; each "Load more" grows the window by this much.
const PAGE_SIZE: i64 = 20;
/// Widest date range the filter will accept, in days.
const MAX_RANGE_DAYS: i64 = 366;

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Inverse of [`days_from_civil`].
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Parse the `YYYY-MM-DD` prefix of a timestamp into a day ordinal.
fn date_ordinal(s: &str) -> Option<i64> {
    let d = s.get(..10).unwrap_or(s);
    let mut it = d.split('-');
    let y = it.next()?.parse().ok()?;
    let m = it.next()?.parse().ok()?;
    let d = it.next()?.parse().ok()?;
    Some(days_from_civil(y, m, d))
}

/// Shift a `YYYY-MM-DD` date by a number of days.
fn shift_days(date: &str, delta: i64) -> String {
    match date_ordinal(date) {
        Some(o) => {
            let (y, m, d) = civil_from_days(o + delta);
            format!("{y:04}-{m:02}-{d:02}")
        }
        None => date.to_string(),
    }
}

/// A single rendered audit row.
fn entry_row(e: ChangeLogEntry) -> AnyView {
    view! {
        <div class="text-xs text-slate-400">
            <span class="text-slate-300">{e.actor}</span> " changed "
            <span class="text-slate-300">{e.field}</span> " from \"" {e.old_value} "\" to \""
            {e.new_value} "\" · " {e.at}
        </div>
    }
    .into_any()
}

/// An independently-fetched, paginated, date-filtered audit log for one entity.
///
/// Mount it only when the user opens the log (e.g. behind a "Show" / "Open change
/// log" toggle) so the request fires on demand rather than with the surrounding
/// page.
#[component]
pub fn ChangeLog(
    /// Whether the `entity_id` names a user or a case.
    scope: AuditScope,
    /// The id of the user or case whose history to show.
    #[prop(into)]
    entity_id: String,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    let entity_id = StoredValue::new(entity_id);

    // Date range (default: the last two weeks).
    let default_to = today();
    let default_from = shift_days(&default_to, -14);
    let from = RwSignal::new(default_from);
    let to = RwSignal::new(default_to);

    // Fetched rows, the total matching the current filter, the size of the
    // window we currently request, and the request lifecycle signals.
    let rows = RwSignal::new(Vec::<ChangeLogEntry>::new());
    let total = RwSignal::new(0i64);
    let window = RwSignal::new(PAGE_SIZE);
    let loading = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    // A local validation error blocks the fetch (and takes precedence).
    let invalid = RwSignal::new(None::<String>);

    // Reset the window to the first page whenever the date range changes, so a
    // new filter starts from the top rather than keeping an old deep offset.
    Effect::new(move |prev: Option<(String, String)>| {
        let range = (from.get(), to.get());
        if prev.as_ref().is_some_and(|p| *p != range) {
            window.set(PAGE_SIZE);
        }
        range
    });

    // Fetch `[0, window)` whenever the range or window changes. Validates the
    // range client-side first (cheap feedback) and only then hits the server.
    Effect::new(move |_| {
        let count = window.get();
        let (f, t) = (from.get(), to.get());
        if !state.is_authenticated() {
            return;
        }

        match (date_ordinal(&f), date_ordinal(&t)) {
            (Some(fo), Some(to_o)) if fo > to_o => {
                invalid.set(Some("From date must be on or before the To date.".into()));
                rows.set(Vec::new());
                total.set(0);
                return;
            }
            (Some(fo), Some(to_o)) if to_o - fo > MAX_RANGE_DAYS => {
                invalid.set(Some(format!(
                    "Date range is too large — choose at most {MAX_RANGE_DAYS} days."
                )));
                rows.set(Vec::new());
                total.set(0);
                return;
            }
            (Some(_), Some(_)) => invalid.set(None),
            _ => {
                invalid.set(Some("Enter a valid From and To date.".into()));
                rows.set(Vec::new());
                total.set(0);
                return;
            }
        }

        loading.set(true);
        error.set(None);
        let id = entity_id.get_value();
        spawn_local(async move {
            match list_audit_page(scope, id, f, t, 0, count).await {
                Ok(page) => {
                    rows.set(page.items);
                    total.set(page.total);
                }
                Err(e) => error.set(Some(err_text(e))),
            }
            loading.set(false);
        });
    });

    let date_input =
        "mt-1 block rounded-xl border border-slate-700 bg-slate-900 px-2 py-1.5 text-sm text-slate-100";

    let body = move || {
        if let Some(msg) = invalid.get() {
            return view! { <p class="text-xs text-rose-300">{msg}</p> }.into_any();
        }
        if let Some(msg) = error.get() {
            return view! { <p class="text-xs text-rose-300">{msg}</p> }.into_any();
        }
        // Show the spinner only on the very first load (no rows yet), so a
        // "Load more" refetch doesn't blank out the already-visible list.
        if loading.get() && rows.with(Vec::is_empty) {
            return view! { <Loading label="Loading change log\u{2026}" /> }.into_any();
        }
        let items = rows.get();
        if items.is_empty() {
            return view! {
                <p class="text-xs text-slate-500">"No changes in this date range."</p>
            }
            .into_any();
        }
        let shown = items.len() as i64;
        let total_n = total.get();
        let list = items.into_iter().map(entry_row).collect_view();
        let footer = if shown < total_n {
            view! {
                <div class="flex items-center gap-3">
                    <button
                        on:click=move |_| window.update(|w| *w += PAGE_SIZE)
                        prop:disabled=move || loading.get()
                        class="rounded-full border border-slate-700 px-2 py-1 text-xs font-medium text-slate-300 hover:bg-slate-800 disabled:opacity-50"
                    >
                        {move || if loading.get() { "Loading\u{2026}" } else { "Load more" }}
                    </button>
                    <span class="text-xs text-slate-500">
                        "Showing " {shown} " of " {total_n}
                    </span>
                </div>
            }
            .into_any()
        } else {
            view! { <p class="text-xs text-slate-500">{total_n} " change(s) in range"</p> }
                .into_any()
        };
        view! {
            <div class="space-y-1">{list}</div>
            {footer}
        }
        .into_any()
    };

    view! {
        <div class="mt-3 space-y-3">
            <div class="flex flex-wrap items-end gap-3">
                <label class="text-xs text-slate-400">
                    "From"
                    <input
                        type="date"
                        class=date_input
                        prop:value=move || from.get()
                        on:change=move |ev| from.set(event_target_value(&ev))
                    />
                </label>
                <label class="text-xs text-slate-400">
                    "To"
                    <input
                        type="date"
                        class=date_input
                        prop:value=move || to.get()
                        on:change=move |ev| to.set(event_target_value(&ev))
                    />
                </label>
                <button
                    on:click=move |_| {
                        let t = today();
                        from.set(shift_days(&t, -14));
                        to.set(t);
                    }
                    class="rounded-full border border-slate-700 px-2 py-1.5 text-xs font-medium text-slate-300 hover:bg-slate-800"
                >
                    "Last 2 weeks"
                </button>
            </div>
            {body}
        </div>
    }
}
