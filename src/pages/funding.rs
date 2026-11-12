//! The funding workspace: grants and the money received against them.
//!
//! Available to every non-client account granted information-management access.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;

use crate::components::change_log::ChangeLog;
use crate::components::guard::require_information_management_access;
use crate::components::layout::Layout;
use crate::components::loading::Loading;
use crate::helpers::format::badge_pill;
use crate::server_fns::audit::AuditScope;
use crate::server_fns::contacts::{search_active_contacts, ActiveContactSummary};
use crate::server_fns::crm::format_cents;
use crate::server_fns::err_text;
use crate::server_fns::funding::{
    list_funding, record_funding, void_funding, FundingFilters, FundingInput, FundingKind,
    FundingRecord,
};
use crate::server_fns::grants::{
    create_grant, list_grants, load_grant, load_grant_totals, search_grant_options, update_grant,
    Grant, GrantFilters, GrantInput, GrantStatus, GrantTotals, ReportingCadence,
};
use crate::server_fns::organizations::{search_active_organizations, ActiveOrganizationSummary};
use crate::state::AppState;

const PANEL: &str = "rounded-card border border-slate-800 bg-slate-900 shadow-card p-5";
const INPUT: &str =
    "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-600 focus:border-primary-500 focus:outline-none";
const LABEL: &str = "text-xs font-medium text-slate-400";

#[component]
pub fn FundingPage() -> impl IntoView {
    let state = expect_context::<AppState>();
    let params = use_params_map();
    require_information_management_access(state, move || {
        let selected_id = params.read().get("id").filter(|id| !id.trim().is_empty());
        view! {
            <Layout title="Funding".to_string()>
                {match selected_id {
                    Some(id) => view! { <GrantDetail grant_id=id /> }.into_any(),
                    None => view! { <FundingWorkspace /> }.into_any(),
                }}
            </Layout>
        }
        .into_any()
    })
}

#[component]
fn FundingWorkspace() -> impl IntoView {
    let totals = RwSignal::new(GrantTotals::default());
    let grants = RwSignal::new(Vec::<Grant>::new());
    let grant_total = RwSignal::new(0i64);
    let grant_window = RwSignal::new(50i64);
    let ledger = RwSignal::new(Vec::<FundingRecord>::new());
    let ledger_total = RwSignal::new(0i64);
    let ledger_window = RwSignal::new(50i64);
    let status_filter = RwSignal::new(String::new());
    let include_voided = RwSignal::new(false);
    let loading = RwSignal::new(true);
    let error = RwSignal::new(String::new());
    let reload = RwSignal::new(0u32);
    let creating_grant = RwSignal::new(false);
    let recording = RwSignal::new(false);

    Effect::new(move |_| {
        let status = GrantStatus::from_slug(&status_filter.get());
        let voided = include_voided.get();
        let grant_limit = grant_window.get();
        let ledger_limit = ledger_window.get();
        reload.track();
        loading.set(true);
        spawn_local(async move {
            let filters = GrantFilters {
                status,
                ..Default::default()
            };
            match list_grants(filters, 0, grant_limit).await {
                Ok(page) => {
                    grants.set(page.items);
                    grant_total.set(page.total);
                    error.set(String::new());
                }
                Err(e) => error.set(err_text(e)),
            }
            if let Ok(t) = load_grant_totals().await {
                totals.set(t);
            }
            let funding_filters = FundingFilters {
                include_voided: voided,
                ..Default::default()
            };
            if let Ok(page) = list_funding(funding_filters, 0, ledger_limit).await {
                ledger.set(page.items);
                ledger_total.set(page.total);
            }
            loading.set(false);
        });
    });

    let summary = move || {
        let t = totals.get();
        let card = |label: &'static str, value: String, hint: String| {
            view! {
                <div class="rounded-lg border border-slate-800 bg-slate-950 p-4">
                    <p class="text-xs font-medium text-slate-500">{label}</p>
                    <p class="mt-1 text-xl font-semibold text-slate-100">{value}</p>
                    <p class="mt-0.5 text-xs text-slate-500">{hint}</p>
                </div>
            }
        };
        let outstanding = t.awarded_cents - t.received_cents;
        view! {
            <div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
                {card("Total awarded", format_cents(t.awarded_cents), "Across every awarded grant".to_string())}
                {card("Received against grants", format_cents(t.received_cents), "Excludes voided records".to_string())}
                {card("Outstanding", format_cents(outstanding), "Awarded but not yet received".to_string())}
                {card("Pipeline", t.prospect_count.to_string(), format!("{} active grants", t.active_count))}
            </div>
        }
    };

    let grant_rows = move || {
        let list = grants.get();
        if list.is_empty() {
            let message = if loading.get() {
                "Loading grants\u{2026}"
            } else {
                "No grants match this filter."
            };
            return view! { <p class="text-sm text-slate-500">{message}</p> }.into_any();
        }
        list.into_iter()
            .map(|grant| {
                let href = format!("/funding/{}", grant.id);
                let status = grant.status;
                let funder = grant.funder_name.clone();
                let awarded = grant
                    .amount_awarded_cents
                    .map(format_cents)
                    .unwrap_or_else(|| "Not awarded".to_string());
                let received = format_cents(grant.received_cents);
                let over = grant.is_overfunded();
                view! {
                    <A href=href attr:class="block rounded-lg border border-slate-800 bg-slate-950 p-3 hover:border-primary-500/40">
                        <div class="flex flex-wrap items-center gap-2">
                            <span class="text-sm font-semibold text-slate-100">{grant.name.clone()}</span>
                            <span class=badge_pill(status.badge_classes())>{status.label()}</span>
                            <Show when=move || over>
                                <span class=badge_pill("bg-rose-500/15 text-rose-300 ring-1 ring-rose-500/30")>"Over-funded"</span>
                            </Show>
                        </div>
                        <p class="mt-1 text-xs text-slate-500">
                            {if funder.is_empty() { "No funder set".to_string() } else { funder }}
                            " \u{b7} awarded " {awarded} " \u{b7} received " {received}
                        </p>
                    </A>
                }
            })
            .collect_view()
            .into_any()
    };

    view! {
        <div class="space-y-6">
            {summary}

            <Show when=move || !error.get().is_empty()>
                <p class="text-sm text-rose-300">{move || error.get()}</p>
            </Show>

            <div class=PANEL>
                <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                    <div>
                        <h2 class="text-lg font-semibold text-slate-100">"Grants"</h2>
                        <p class="mt-1 text-sm text-slate-500">"Awards sought and received from funders."</p>
                    </div>
                    <button
                        type="button"
                        on:click=move |_| creating_grant.update(|c| *c = !*c)
                        class="shrink-0 rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600"
                    >
                        {move || if creating_grant.get() { "Cancel" } else { "+ New grant" }}
                    </button>
                </div>

                <Show when=move || creating_grant.get()>
                    <div class="mt-4 border-t border-slate-800 pt-4">
                        <GrantForm
                            grant=None
                            on_saved=Callback::new(move |_id: String| {
                                creating_grant.set(false);
                                reload.update(|r| *r += 1);
                            })
                        />
                    </div>
                </Show>

                <label class="mt-4 block max-w-xs">
                    <span class=LABEL>"Status"</span>
                    <select
                        class=INPUT
                        prop:value=move || status_filter.get()
                        on:change=move |e| {
                            grant_window.set(50);
                            status_filter.set(event_target_value(&e));
                        }
                    >
                        <option value="">"Any status"</option>
                        {GrantStatus::ALL
                            .iter()
                            .map(|s| view! { <option value=s.slug()>{s.label()}</option> })
                            .collect_view()}
                    </select>
                </label>

                <div class="mt-4 space-y-2">{grant_rows}</div>
                <div class="mt-4 flex items-center justify-between text-xs text-slate-500">
                    <span>"Showing " {move || grants.get().len()} " of " {move || grant_total.get()} " grants"</span>
                    <Show when=move || (grants.get().len() as i64) < grant_total.get()>
                        <button type="button" on:click=move |_| grant_window.update(|value| *value += 50)
                            class="rounded-full border border-slate-700 px-3 py-1.5 text-sm text-slate-200 hover:bg-slate-800">
                            "Load more grants"
                        </button>
                    </Show>
                </div>
            </div>

            <div class=PANEL>
                <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                    <div>
                        <h2 class="text-lg font-semibold text-slate-100">"Funding ledger"</h2>
                        <p class="mt-1 text-sm text-slate-500">
                            "Money received. Corrections are made by voiding a record, never by deleting it."
                        </p>
                    </div>
                    <button
                        type="button"
                        on:click=move |_| recording.update(|r| *r = !*r)
                        class="shrink-0 rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600"
                    >
                        {move || if recording.get() { "Cancel" } else { "+ Record funding" }}
                    </button>
                </div>

                <Show when=move || recording.get()>
                    <div class="mt-4 border-t border-slate-800 pt-4">
                        <FundingForm
                            on_saved=Callback::new(move |_id: String| {
                                recording.set(false);
                                reload.update(|r| *r += 1);
                            })
                        />
                    </div>
                </Show>

                <label class="mt-4 flex items-center gap-2 text-sm text-slate-300">
                    <input
                        type="checkbox"
                        class="h-4 w-4 rounded border-slate-700 bg-slate-950"
                        prop:checked=move || include_voided.get()
                        on:change=move |e| {
                            ledger_window.set(50);
                            include_voided.set(event_target_checked(&e));
                        }
                    />
                    "Show voided records"
                </label>

                <div class="mt-3 space-y-2">
                    <FundingList records=ledger on_changed=Callback::new(move |_: ()| reload.update(|r| *r += 1)) />
                </div>
                <div class="mt-4 flex items-center justify-between text-xs text-slate-500">
                    <span>"Showing " {move || ledger.get().len()} " of " {move || ledger_total.get()} " funding records"</span>
                    <Show when=move || (ledger.get().len() as i64) < ledger_total.get()>
                        <button type="button" on:click=move |_| ledger_window.update(|value| *value += 50)
                            class="rounded-full border border-slate-700 px-3 py-1.5 text-sm text-slate-200 hover:bg-slate-800">
                            "Load more funding"
                        </button>
                    </Show>
                </div>
            </div>
        </div>
    }
}

/// The shared ledger renderer, used by the workspace and the grant detail page.
#[component]
fn FundingList(records: RwSignal<Vec<FundingRecord>>, on_changed: Callback<()>) -> impl IntoView {
    let voiding = RwSignal::new(String::new());
    let reason = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());

    let confirm_void = move |_| {
        let id = voiding.get_untracked();
        let why = reason.get_untracked();
        if id.is_empty() {
            return;
        }
        spawn_local(async move {
            match void_funding(id, why).await {
                Ok(()) => {
                    voiding.set(String::new());
                    reason.set(String::new());
                    error.set(String::new());
                    on_changed.run(());
                }
                Err(e) => error.set(err_text(e)),
            }
        });
    };

    view! {
        <div class="space-y-2">
            <Show when=move || !error.get().is_empty()>
                <p class="text-sm text-rose-300" role="alert">{move || error.get()}</p>
            </Show>
            {move || {
                let list = records.get();
                if list.is_empty() {
                    return view! { <p class="text-sm text-slate-500">"No funding recorded yet."</p> }
                        .into_any();
                }
                list.into_iter()
                    .map(|record| {
                        let id = record.id.clone();
                        let kind = record.kind;
                        let voided = record.voided;
                        let amount = format_cents(record.amount_cents);
                        let source = record.source_label();
                        let void_reason = record.void_reason.clone();
                        let voided_by = record.voided_by.clone();
                        let voided_at = record.voided_at.clone();
                        let reference = record.reference.clone();
                        let notes = record.notes.clone();
                        let grant_name = record.grant_name.clone();
                        let created_at = record.created_at.clone();
                        let recorded_by = record.recorded_by.clone();
                        let audit_id = record.id.clone();
                        let being_voided = {
                            let id = id.clone();
                            move || voiding.get() == id
                        };
                        let start_void = {
                            let id = id.clone();
                            move |_| {
                                voiding.set(id.clone());
                                reason.set(String::new());
                            }
                        };
                        view! {
                            <div class="rounded-lg border border-slate-800 bg-slate-950 p-3">
                                <div class="flex flex-wrap items-center justify-between gap-2">
                                    <div class="flex flex-wrap items-center gap-2">
                                        <span class=move || {
                                            if voided {
                                                "text-sm font-semibold text-slate-500 line-through"
                                            } else {
                                                "text-sm font-semibold text-slate-100"
                                            }
                                        }>{amount}</span>
                                        <span class=badge_pill(kind.badge_classes())>{kind.label()}</span>
                                        <Show when=move || voided>
                                            <span class=badge_pill("bg-rose-500/15 text-rose-300 ring-1 ring-rose-500/30")>"Voided"</span>
                                        </Show>
                                    </div>
                                    <Show when=move || !voided>
                                        <button
                                            type="button"
                                            on:click=start_void.clone()
                                            class="rounded-full border border-slate-700 px-2 py-1 text-xs font-medium text-slate-400 hover:bg-slate-800"
                                        >
                                            "Void"
                                        </button>
                                    </Show>
                                </div>
                                <p class="mt-1 text-xs text-slate-500">
                                    {source} " \u{b7} " {record.received_on.clone()}
                                    " \u{b7} recorded by " {record.recorded_by.clone()}
                                    {if reference.is_empty() { String::new() } else { format!(" \u{b7} ref {reference}") }}
                                </p>
                                <Show when=move || voided>
                                    <p class="mt-1 text-xs text-rose-300">
                                        "Voided " {voided_at.clone()} " by " {voided_by.clone()} ": " {void_reason.clone()}
                                    </p>
                                </Show>
                                <details class="mt-2 rounded-lg border border-slate-800 px-3 py-2">
                                    <summary class="cursor-pointer text-xs font-medium text-slate-300">"Full record and change log"</summary>
                                    <dl class="mt-2 grid gap-2 text-xs sm:grid-cols-2">
                                        <div><dt class="text-slate-500">"Funding ID"</dt><dd class="text-slate-200">{id.clone()}</dd></div>
                                        <div><dt class="text-slate-500">"Grant"</dt><dd class="text-slate-200">{if grant_name.is_empty() { "Not linked".to_string() } else { grant_name.clone() }}</dd></div>
                                        <div><dt class="text-slate-500">"Recorded"</dt><dd class="text-slate-200">{created_at.clone()} " by " {recorded_by.clone()}</dd></div>
                                        <div><dt class="text-slate-500">"Reference"</dt><dd class="text-slate-200">{if reference.is_empty() { "Not provided".to_string() } else { reference.clone() }}</dd></div>
                                        <div class="sm:col-span-2"><dt class="text-slate-500">"Notes"</dt><dd class="whitespace-pre-wrap text-slate-200">{if notes.is_empty() { "Not provided".to_string() } else { notes.clone() }}</dd></div>
                                    </dl>
                                    <div class="mt-3 border-t border-slate-800 pt-3">
                                        <ChangeLog scope=AuditScope::Funding entity_id=audit_id />
                                    </div>
                                </details>
                                <Show when=being_voided.clone()>
                                    <div class="mt-2 flex flex-wrap gap-2">
                                        <input
                                            class=INPUT
                                            placeholder="Why is this being voided?"
                                            prop:value=move || reason.get()
                                            on:input=move |e| reason.set(event_target_value(&e))
                                        />
                                        <button
                                            type="button"
                                            on:click=confirm_void
                                            class="rounded-full bg-rose-500/80 px-3 py-1.5 text-sm font-semibold text-white hover:bg-rose-500"
                                        >
                                            "Confirm void"
                                        </button>
                                        <button
                                            type="button"
                                            on:click=move |_| voiding.set(String::new())
                                            class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-300 hover:bg-slate-800"
                                        >
                                            "Cancel"
                                        </button>
                                    </div>
                                </Show>
                            </div>
                        }
                    })
                    .collect_view()
                    .into_any()
            }}
        </div>
    }
}

#[component]
fn GrantDetail(grant_id: String) -> impl IntoView {
    let id = StoredValue::new(grant_id);
    let grant = RwSignal::new(None::<Grant>);
    let ledger = RwSignal::new(Vec::<FundingRecord>::new());
    let loading = RwSignal::new(true);
    let error = RwSignal::new(String::new());
    let editing = RwSignal::new(false);
    let reload = RwSignal::new(0u32);

    Effect::new(move |_| {
        reload.track();
        loading.set(true);
        spawn_local(async move {
            match load_grant(id.get_value()).await {
                Ok(found) => {
                    grant.set(found);
                    error.set(String::new());
                }
                Err(e) => error.set(err_text(e)),
            }
            let filters = FundingFilters {
                grant_id: id.get_value(),
                include_voided: true,
                ..Default::default()
            };
            if let Ok(page) = list_funding(filters, 0, 100).await {
                ledger.set(page.items);
            }
            loading.set(false);
        });
    });

    view! {
        <div class="space-y-6">
            <Show when=move || loading.get() && grant.get().is_none()>
                <div class=PANEL><Loading label="Loading grant\u{2026}" /></div>
            </Show>
            <Show when=move || !error.get().is_empty()>
                <p class="text-sm text-rose-300">{move || error.get()}</p>
            </Show>

            {move || {
                let Some(g) = grant.get() else {
                    return ().into_any();
                };
                let status = g.status;
                let awarded = g.amount_awarded_cents;
                let remaining = g.remaining_cents();
                let over = g.is_overfunded();
                let received = g.received_cents;
                let detail = g.clone();
                view! {
                    <div class=PANEL>
                        <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                            <div class="flex flex-wrap items-center gap-2">
                                <h2 class="text-lg font-semibold text-slate-100">{g.name.clone()}</h2>
                                <span class=badge_pill(status.badge_classes())>{status.label()}</span>
                            </div>
                            <button
                                type="button"
                                on:click=move |_| editing.update(|e| *e = !*e)
                                class="shrink-0 rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800"
                            >
                                {move || if editing.get() { "Cancel" } else { "Edit" }}
                            </button>
                        </div>

                        <Show
                            when=move || editing.get()
                            fallback=move || {
                                let g = detail.clone();
                                let period = if g.period_start.is_empty() {
                                    "Not set".to_string()
                                } else {
                                    format!("{} to {}", g.period_start, g.period_end)
                                };
                                view! {
                                    <div>
                                        <div class="mt-4 grid gap-3 sm:grid-cols-3">
                                            <div class="rounded-lg border border-slate-800 bg-slate-950 p-3">
                                                <p class="text-xs text-slate-500">"Awarded"</p>
                                                <p class="mt-1 text-lg font-semibold text-slate-100">
                                                    {awarded.map(format_cents).unwrap_or_else(|| "\u{2014}".to_string())}
                                                </p>
                                            </div>
                                            <div class="rounded-lg border border-slate-800 bg-slate-950 p-3">
                                                <p class="text-xs text-slate-500">"Received"</p>
                                                <p class="mt-1 text-lg font-semibold text-emerald-300">{format_cents(received)}</p>
                                            </div>
                                            <div class="rounded-lg border border-slate-800 bg-slate-950 p-3">
                                                <p class="text-xs text-slate-500">"Remaining"</p>
                                                <p class=move || {
                                                    if over {
                                                        "mt-1 text-lg font-semibold text-rose-300"
                                                    } else {
                                                        "mt-1 text-lg font-semibold text-slate-100"
                                                    }
                                                }>
                                                    {remaining.map(format_cents).unwrap_or_else(|| "\u{2014}".to_string())}
                                                </p>
                                            </div>
                                        </div>
                                        <dl class="mt-4 grid gap-x-6 sm:grid-cols-2">
                                            <GrantRow label="Funder" value=g.funder_name.clone() />
                                            <GrantRow label="Program officer" value=g.program_officer_name.clone() />
                                            <GrantRow label="Period" value=period />
                                            <GrantRow label="Reporting" value=g.reporting_cadence.label().to_string() />
                                            <GrantRow label="Applied" value=g.application_date.clone() />
                                            <GrantRow label="Decision" value=g.decision_date.clone() />
                                            <GrantRow label="Purpose" value=g.purpose.clone() />
                                            <GrantRow label="Notes" value=g.notes.clone() />
                                        </dl>
                                    </div>
                                }
                            }
                        >
                            <div class="mt-4 border-t border-slate-800 pt-4">
                                <GrantForm
                                    grant=grant.get()
                                    on_saved=Callback::new(move |_id: String| {
                                        editing.set(false);
                                        reload.update(|r| *r += 1);
                                    })
                                />
                            </div>
                        </Show>
                    </div>
                }
                .into_any()
            }}

            <div class=PANEL>
                <h3 class="text-sm font-semibold text-slate-200">"Funding received"</h3>
                <div class="mt-3">
                    <FundingList records=ledger on_changed=Callback::new(move |_: ()| reload.update(|r| *r += 1)) />
                </div>
            </div>

            <div class=PANEL>
                <h3 class="text-sm font-semibold text-slate-200">"Change log"</h3>
                <div class="mt-3">
                    <ChangeLog scope=AuditScope::Grant entity_id=id.get_value() />
                </div>
            </div>

            <A href="/funding" attr:class="inline-block text-sm text-primary-400 hover:text-primary-300">
                "\u{2190} Back to funding"
            </A>
        </div>
    }
}

#[component]
fn GrantRow(label: &'static str, value: String) -> impl IntoView {
    let empty = value.trim().is_empty();
    view! {
        <div class="border-b border-slate-800 py-2">
            <dt class="text-xs font-medium text-slate-500">{label}</dt>
            <dd class=if empty {
                "mt-1 text-sm italic text-slate-500"
            } else {
                "mt-1 whitespace-pre-wrap text-sm text-slate-200"
            }>{if empty { "Not set".to_string() } else { value }}</dd>
        </div>
    }
}

#[component]
fn GrantForm(grant: Option<Grant>, on_saved: Callback<String>) -> impl IntoView {
    let existing_id = grant.as_ref().map(|g| g.id.clone());
    let editing = StoredValue::new(existing_id.clone());
    let seed = grant.unwrap_or_default();

    let name = RwSignal::new(seed.name.clone());
    let status = RwSignal::new(seed.status.slug().to_string());
    let funder = RwSignal::new(seed.funder_organization_id.clone());
    let officer = RwSignal::new(seed.program_officer_contact_id.clone());
    let requested = RwSignal::new(
        seed.amount_requested_cents
            .map(|c| format!("{}.{:02}", c / 100, c % 100))
            .unwrap_or_default(),
    );
    let awarded = RwSignal::new(
        seed.amount_awarded_cents
            .map(|c| format!("{}.{:02}", c / 100, c % 100))
            .unwrap_or_default(),
    );
    let application_date = RwSignal::new(seed.application_date.clone());
    let decision_date = RwSignal::new(seed.decision_date.clone());
    let period_start = RwSignal::new(seed.period_start.clone());
    let period_end = RwSignal::new(seed.period_end.clone());
    let purpose = RwSignal::new(seed.purpose.clone());
    let cadence = RwSignal::new(seed.reporting_cadence.slug().to_string());
    let notes = RwSignal::new(seed.notes.clone());
    let error = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    let submit = move |_| {
        if busy.get_untracked() {
            return;
        }
        let input = GrantInput {
            name: name.get_untracked(),
            status: GrantStatus::from_slug(&status.get_untracked()).unwrap_or_default(),
            funder_organization_id: funder.get_untracked(),
            program_officer_contact_id: officer.get_untracked(),
            amount_requested: requested.get_untracked(),
            amount_awarded: awarded.get_untracked(),
            application_date: application_date.get_untracked(),
            decision_date: decision_date.get_untracked(),
            period_start: period_start.get_untracked(),
            period_end: period_end.get_untracked(),
            purpose: purpose.get_untracked(),
            reporting_cadence: ReportingCadence::from_slug(&cadence.get_untracked())
                .unwrap_or_default(),
            notes: notes.get_untracked(),
        };
        if let Err(message) = input.validate() {
            error.set(message);
            return;
        }
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = match editing.get_value() {
                Some(id) => update_grant(id.clone(), input).await.map(|()| id),
                None => create_grant(input).await,
            };
            match result {
                Ok(id) => on_saved.run(id),
                Err(e) => error.set(err_text(e)),
            }
            busy.set(false);
        });
    };

    let field = move |label: &'static str, signal: RwSignal<String>, placeholder: &'static str| {
        view! {
            <label class="block">
                <span class=LABEL>{label}</span>
                <input
                    class=INPUT
                    placeholder=placeholder
                    prop:value=move || signal.get()
                    on:input=move |e| signal.set(event_target_value(&e))
                />
            </label>
        }
    };

    view! {
        <div class="space-y-4">
            <div class="grid gap-3 sm:grid-cols-2">
                {field("Grant name", name, "")}
                <label class="block">
                    <span class=LABEL>"Status"</span>
                    <select
                        class=INPUT
                        prop:value=move || status.get()
                        on:change=move |e| status.set(event_target_value(&e))
                    >
                        {GrantStatus::ALL
                            .iter()
                            .map(|s| view! { <option value=s.slug()>{s.label()}</option> })
                            .collect_view()}
                    </select>
                </label>
                <OrganizationPicker
                    label="Funder"
                    selected=funder
                    initial_label=seed.funder_name.clone()
                />
                <ContactPicker
                    label="Program officer"
                    selected=officer
                    initial_label=seed.program_officer_name.clone()
                />
                {field("Amount requested", requested, "e.g. 25000")}
                {field("Amount awarded", awarded, "Required once awarded")}
                {field("Application date", application_date, "YYYY-MM-DD")}
                {field("Decision date", decision_date, "YYYY-MM-DD")}
                {field("Period start", period_start, "YYYY-MM-DD")}
                {field("Period end", period_end, "YYYY-MM-DD")}
                <label class="block">
                    <span class=LABEL>"Reporting"</span>
                    <select
                        class=INPUT
                        prop:value=move || cadence.get()
                        on:change=move |e| cadence.set(event_target_value(&e))
                    >
                        {ReportingCadence::ALL
                            .iter()
                            .map(|c| view! { <option value=c.slug()>{c.label()}</option> })
                            .collect_view()}
                    </select>
                </label>
            </div>
            <label class="block">
                <span class=LABEL>"Purpose"</span>
                <textarea
                    class=INPUT
                    rows="2"
                    prop:value=move || purpose.get()
                    on:input=move |e| purpose.set(event_target_value(&e))
                />
            </label>
            <label class="block">
                <span class=LABEL>"Notes"</span>
                <textarea
                    class=INPUT
                    rows="2"
                    prop:value=move || notes.get()
                    on:input=move |e| notes.set(event_target_value(&e))
                />
            </label>
            <Show when=move || !error.get().is_empty()>
                <p class="text-sm text-rose-300" role="alert">{move || error.get()}</p>
            </Show>
            <button
                type="button"
                on:click=submit
                prop:disabled=move || busy.get()
                class="rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
            >
                {move || {
                    if busy.get() {
                        "Saving\u{2026}"
                    } else if existing_id.is_some() {
                        "Save changes"
                    } else {
                        "Create grant"
                    }
                }}
            </button>
        </div>
    }
}

#[component]
fn FundingForm(on_saved: Callback<String>) -> impl IntoView {
    let kind = RwSignal::new(FundingKind::default().slug().to_string());
    let amount = RwSignal::new(String::new());
    let received_on = RwSignal::new(String::new());
    let grant_id = RwSignal::new(String::new());
    let organization_id = RwSignal::new(String::new());
    let contact_id = RwSignal::new(String::new());
    let reference = RwSignal::new(String::new());
    let notes = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    let submit = move |_| {
        if busy.get_untracked() {
            return;
        }
        let input = FundingInput {
            kind: FundingKind::from_slug(&kind.get_untracked()).unwrap_or_default(),
            amount: amount.get_untracked(),
            received_on: received_on.get_untracked(),
            grant_id: grant_id.get_untracked(),
            source_organization_id: organization_id.get_untracked(),
            source_contact_id: contact_id.get_untracked(),
            reference: reference.get_untracked(),
            notes: notes.get_untracked(),
        };
        if let Err(message) = input.validate() {
            error.set(message);
            return;
        }
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match record_funding(input).await {
                Ok(id) => {
                    amount.set(String::new());
                    reference.set(String::new());
                    notes.set(String::new());
                    on_saved.run(id);
                }
                Err(e) => error.set(err_text(e)),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="space-y-4">
            <div class="grid gap-3 sm:grid-cols-2">
                <label class="block">
                    <span class=LABEL>"Kind"</span>
                    <select
                        class=INPUT
                        prop:value=move || kind.get()
                        on:change=move |e| kind.set(event_target_value(&e))
                    >
                        {FundingKind::ALL
                            .iter()
                            .map(|k| view! { <option value=k.slug()>{k.label()}</option> })
                            .collect_view()}
                    </select>
                </label>
                <label class="block">
                    <span class=LABEL>"Amount"</span>
                    <input
                        class=INPUT
                        placeholder="e.g. 2500.00"
                        prop:value=move || amount.get()
                        on:input=move |e| amount.set(event_target_value(&e))
                    />
                </label>
                <label class="block">
                    <span class=LABEL>"Received on"</span>
                    <input
                        class=INPUT
                        placeholder="YYYY-MM-DD"
                        prop:value=move || received_on.get()
                        on:input=move |e| received_on.set(event_target_value(&e))
                    />
                </label>
                <GrantPicker label="Against grant" selected=grant_id />
                <OrganizationPicker label="From organization" selected=organization_id />
                <ContactPicker label="From person" selected=contact_id />
            </div>
            <label class="block">
                <span class=LABEL>"Reference"</span>
                <input
                    class=INPUT
                    placeholder="Cheque or transfer number"
                    prop:value=move || reference.get()
                    on:input=move |e| reference.set(event_target_value(&e))
                />
            </label>
            <label class="block">
                <span class=LABEL>"Notes"</span>
                <textarea
                    class=INPUT
                    rows="3"
                    prop:value=move || notes.get()
                    on:input=move |e| notes.set(event_target_value(&e))
                />
            </label>
            <Show when=move || !error.get().is_empty()>
                <p class="text-sm text-rose-300" role="alert">{move || error.get()}</p>
            </Show>
            <button
                type="button"
                on:click=submit
                prop:disabled=move || busy.get()
                class="rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
            >
                {move || if busy.get() { "Saving\u{2026}" } else { "Record funding" }}
            </button>
        </div>
    }
}

#[component]
fn OrganizationPicker(
    label: &'static str,
    selected: RwSignal<String>,
    #[prop(optional)] initial_label: String,
) -> impl IntoView {
    let query = RwSignal::new(initial_label);
    let debounced = RwSignal::new(String::new());
    let results = RwSignal::new(Vec::<ActiveOrganizationSummary>::new());
    let mut search = debounce(std::time::Duration::from_millis(300), move |value| {
        debounced.set(value)
    });
    Effect::new(move |_| {
        let value = debounced.get();
        spawn_local(async move {
            match search_active_organizations(value).await {
                Ok(items) => results.set(items),
                Err(_) => results.set(Vec::new()),
            }
        });
    });
    view! {
        <label class="block">
            <span class=LABEL>{label}</span>
            <input type="search" class=INPUT placeholder="Search active organizations"
                prop:value=move || query.get()
                on:input=move |event| {
                    selected.set(String::new());
                    let value = event_target_value(&event);
                    query.set(value.clone());
                    search(value);
                } />
            <Show when=move || selected.get().is_empty() && !query.get().is_empty()>
                <div class="mt-1 max-h-32 overflow-y-auto rounded-lg border border-slate-700 bg-slate-950">
                    {move || results.get().into_iter().map(|item| {
                        let id = item.id.clone();
                        let name = item.name.clone();
                        view! { <button type="button" class="block w-full px-3 py-1.5 text-left text-xs text-slate-200 hover:bg-slate-800"
                            on:click=move |_| { selected.set(id.clone()); query.set(name.clone()); }>{item.name}</button> }
                    }).collect_view()}
                </div>
            </Show>
            <Show when=move || !query.get().is_empty()>
                <button type="button" class="mt-1 text-xs text-slate-500 hover:text-slate-300"
                    on:click=move |_| { selected.set(String::new()); query.set(String::new()); results.set(Vec::new()); }>"Clear"</button>
            </Show>
        </label>
    }
}

#[component]
fn ContactPicker(
    label: &'static str,
    selected: RwSignal<String>,
    #[prop(optional)] initial_label: String,
) -> impl IntoView {
    let query = RwSignal::new(initial_label);
    let debounced = RwSignal::new(String::new());
    let results = RwSignal::new(Vec::<ActiveContactSummary>::new());
    let mut search = debounce(std::time::Duration::from_millis(300), move |value| {
        debounced.set(value)
    });
    Effect::new(move |_| {
        let value = debounced.get();
        spawn_local(async move {
            match search_active_contacts(value).await {
                Ok(items) => results.set(items),
                Err(_) => results.set(Vec::new()),
            }
        });
    });
    view! {
        <label class="block">
            <span class=LABEL>{label}</span>
            <input type="search" class=INPUT placeholder="Search active people"
                prop:value=move || query.get()
                on:input=move |event| {
                    selected.set(String::new());
                    let value = event_target_value(&event);
                    query.set(value.clone());
                    search(value);
                } />
            <Show when=move || selected.get().is_empty() && !query.get().is_empty()>
                <div class="mt-1 max-h-32 overflow-y-auto rounded-lg border border-slate-700 bg-slate-950">
                    {move || results.get().into_iter().map(|item| {
                        let id = item.id.clone();
                        let label = item.label.clone();
                        view! { <button type="button" class="block w-full px-3 py-1.5 text-left text-xs text-slate-200 hover:bg-slate-800"
                            on:click=move |_| { selected.set(id.clone()); query.set(label.clone()); }>{item.label}</button> }
                    }).collect_view()}
                </div>
            </Show>
            <Show when=move || !query.get().is_empty()>
                <button type="button" class="mt-1 text-xs text-slate-500 hover:text-slate-300"
                    on:click=move |_| { selected.set(String::new()); query.set(String::new()); results.set(Vec::new()); }>"Clear"</button>
            </Show>
        </label>
    }
}

#[component]
fn GrantPicker(label: &'static str, selected: RwSignal<String>) -> impl IntoView {
    let query = RwSignal::new(String::new());
    let debounced = RwSignal::new(String::new());
    let results = RwSignal::new(Vec::<(String, String)>::new());
    let mut search = debounce(std::time::Duration::from_millis(300), move |value| {
        debounced.set(value)
    });
    Effect::new(move |_| {
        let value = debounced.get();
        spawn_local(async move {
            match search_grant_options(value).await {
                Ok(items) => results.set(items),
                Err(_) => results.set(Vec::new()),
            }
        });
    });
    view! {
        <label class="block">
            <span class=LABEL>{label}</span>
            <input type="search" class=INPUT placeholder="Search grants"
                prop:value=move || query.get()
                on:input=move |event| {
                    selected.set(String::new());
                    let value = event_target_value(&event);
                    query.set(value.clone());
                    search(value);
                } />
            <Show when=move || selected.get().is_empty() && !query.get().is_empty()>
                <div class="mt-1 max-h-32 overflow-y-auto rounded-lg border border-slate-700 bg-slate-950">
                    {move || results.get().into_iter().map(|(id, name)| {
                        let selected_name = name.clone();
                        view! { <button type="button" class="block w-full px-3 py-1.5 text-left text-xs text-slate-200 hover:bg-slate-800"
                            on:click=move |_| { selected.set(id.clone()); query.set(selected_name.clone()); }>{name}</button> }
                    }).collect_view()}
                </div>
            </Show>
            <Show when=move || !query.get().is_empty()>
                <button type="button" class="mt-1 text-xs text-slate-500 hover:text-slate-300"
                    on:click=move |_| { selected.set(String::new()); query.set(String::new()); results.set(Vec::new()); }>"Clear"</button>
            </Show>
        </label>
    }
}
