//! A case's number, shown beside the client name.
//!
//! Two cases can carry the same client name (a duplicate entry, or two family
//! members), so the name alone does not say which record you are looking at.
//! The case number is the case id (`c-5001`): it is unique, never changes,
//! already matches in the case search box, and is only ever shown to someone
//! who can already see the case.

use leptos::prelude::*;

/// A small monospace case number, e.g. `c-5001`.
#[component]
pub fn CaseNumber(
    /// The case id.
    #[prop(into)]
    id: String,
) -> impl IntoView {
    let label = format!("Case number {id}");
    view! {
        <span
            class="shrink-0 font-mono text-xs tabular-nums text-slate-500"
            title="Case number"
            aria-label=label
        >
            {id}
        </span>
    }
}
