//! Administrator UI for selecting contacts and monitoring one mail campaign.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;

use crate::components::category_picker::CategoryPicker;
use crate::components::guard::{require_information_management_access, require_operations_admin};
use crate::components::layout::Layout;
use crate::components::property_filters::PropertyFilterBar;
use crate::server_fns::contact_directory::{list_contact_categories, ContactCategory};
use crate::server_fns::contact_mail::{
    cancel_contact_mail_task, list_contact_mail_candidates, load_contact_mail_task,
    preview_blocked_recipients, start_contact_mail_task, BlockedRecipient, ContactMailCandidate,
    ContactMailFilters, ContactMailSelection, ContactMailTask, MAX_MAIL_BODY, MAX_MAIL_SUBJECT,
};
use crate::server_fns::contacts::ContactType;
use crate::server_fns::err_text;
use crate::server_fns::organizations::{list_organizations, OrganizationFilters};
use crate::server_fns::property_filters::{PropertyFacetScope, PropertyFilter, PropertySubject};
use crate::state::AppState;

const INPUT: &str = "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40 disabled:cursor-not-allowed disabled:opacity-50";
const LABEL: &str = "mb-1 block text-xs font-medium text-slate-400";
const PANEL: &str = "rounded-card border border-slate-800 bg-slate-900 shadow-card p-5";

#[component]
pub fn ContactMailPage() -> impl IntoView {
    let state = expect_context::<AppState>();
    require_operations_admin(state, move || {
        require_information_management_access(state, move || {
            view! {
                <Layout title="Send contact mail".to_string()>
                    <ContactMailWorkspace />
                </Layout>
            }
            .into_any()
        })
    })
}

#[component]
fn ContactMailWorkspace() -> impl IntoView {
    let task = RwSignal::new(None::<ContactMailTask>);
    let candidates = RwSignal::new(Vec::<ContactMailCandidate>::new());
    let total = RwSignal::new(0i64);
    let categories = RwSignal::new(Vec::<ContactCategory>::new());
    let organizations = RwSignal::new(Vec::<(String, String)>::new());
    let loading = RwSignal::new(true);
    let search_generation = RwSignal::new(0u64);
    let task_poll_generation = RwSignal::new(0u64);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let poll_error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let offset = RwSignal::new(0i64);

    let query = RwSignal::new(String::new());
    let contact_type = RwSignal::new(String::new());
    let organization_id = RwSignal::new(String::new());
    let category_ids = RwSignal::new(Vec::<String>::new());
    let property_filters = RwSignal::new(Vec::<PropertyFilter>::new());
    let applied_filters = RwSignal::new(ContactMailFilters::default());

    let selected_ids = RwSignal::new(Vec::<String>::new());
    let excluded_ids = RwSignal::new(Vec::<String>::new());
    let all_matching = RwSignal::new(false);
    let subject = RwSignal::new(String::new());
    let body = RwSignal::new(String::new());
    let blocked_recipients = RwSignal::new(Vec::<BlockedRecipient>::new());

    let active = Signal::derive(move || task.get().is_some_and(|task| task.status.is_active()));
    let selected_count = Signal::derive(move || {
        if all_matching.get() {
            (total.get() - excluded_ids.get().len() as i64).max(0)
        } else {
            selected_ids.get().len() as i64
        }
    });

    Effect::new(move |_| {
        spawn_local(async move {
            let categories_result = list_contact_categories().await;
            let organizations_result = list_organizations(
                OrganizationFilters {
                    include_archived: true,
                    ..Default::default()
                },
                0,
                200,
            )
            .await;
            if let Ok(items) = categories_result {
                categories.set(items);
            }
            if let Ok(page) = organizations_result {
                organizations.set(
                    page.items
                        .into_iter()
                        .map(|organization| (organization.id, organization.name))
                        .collect(),
                );
            }
        });
    });

    Effect::new(move |_| {
        let filters = applied_filters.get();
        let page_offset = offset.get();
        search_generation.update(|generation| *generation += 1);
        let generation = search_generation.get_untracked();
        loading.set(true);
        spawn_local(async move {
            match list_contact_mail_candidates(filters, page_offset, 50).await {
                Ok(page) if search_generation.get_untracked() == generation => {
                    candidates.set(page.items);
                    total.set(page.total);
                    error.set(String::new());
                }
                Err(server_error) if search_generation.get_untracked() == generation => {
                    error.set(err_text(server_error));
                }
                _ => return,
            }
            loading.set(false);
        });
    });

    let refresh_task = move || {
        task_poll_generation.update(|generation| *generation += 1);
        let generation = task_poll_generation.get_untracked();
        spawn_local(async move {
            match load_contact_mail_task().await {
                Ok(latest) if task_poll_generation.get_untracked() == generation => {
                    task.set(latest);
                    poll_error.set(String::new());
                }
                Err(server_error) if task_poll_generation.get_untracked() == generation => {
                    poll_error.set(err_text(server_error));
                }
                _ => {}
            }
        });
    };
    Effect::new(move |_| {
        refresh_task();
        if let Ok(handle) =
            set_interval_with_handle(refresh_task, std::time::Duration::from_secs(3))
        {
            on_cleanup(move || handle.clear());
        }
    });

    let apply_filters = move |_| {
        offset.set(0);
        if all_matching.get_untracked() {
            all_matching.set(false);
            excluded_ids.set(Vec::new());
        }
        applied_filters.set(ContactMailFilters {
            query: query.get_untracked(),
            category_ids: category_ids.get_untracked(),
            contact_type: ContactType::from_slug(&contact_type.get_untracked()),
            organization_id: organization_id.get_untracked(),
            property_filters: property_filters.get_untracked(),
        });
    };

    // Facet counts describe the recipients actually listed, and only mailable
    // people can ever be listed here.
    let facet_scope = Signal::derive(move || {
        let applied = applied_filters.get();
        PropertyFacetScope {
            keyword: applied.query,
            category_ids: applied.category_ids,
            contact_type: applied
                .contact_type
                .map(|kind| kind.slug().to_string())
                .unwrap_or_default(),
            organization_id: applied.organization_id,
            mailable_only: true,
            ..Default::default()
        }
    });

    // A chip commits on its own Apply. Fold it straight into the applied filters
    // and drop the selection, so a tick made before the change cannot survive it.
    Effect::new(move |previous: Option<Vec<PropertyFilter>>| {
        let current = property_filters.get();
        if previous.is_some_and(|previous| previous != current) {
            offset.set(0);
            all_matching.set(false);
            selected_ids.set(Vec::new());
            excluded_ids.set(Vec::new());
            applied_filters.update(|filters| filters.property_filters = current.clone());
        }
        current
    });

    let clear_selection = move |_| {
        all_matching.set(false);
        selected_ids.set(Vec::new());
        excluded_ids.set(Vec::new());
    };

    let select_visible = move |_| {
        let ids = candidates
            .get_untracked()
            .into_iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>();
        if all_matching.get_untracked() {
            excluded_ids.update(|excluded| excluded.retain(|id| !ids.contains(id)));
        } else {
            selected_ids.update(|selected| {
                for id in ids {
                    if !selected.contains(&id) {
                        selected.push(id);
                    }
                }
            });
        }
    };

    let launch = move |_| {
        if busy.get_untracked() || active.get_untracked() {
            return;
        }
        let count = selected_count.get_untracked();
        if count == 0 {
            error.set("Choose at least one contact.".to_string());
            return;
        }
        let launch_subject = subject.get_untracked();
        let launch_body = body.get_untracked();
        let selection = ContactMailSelection {
            all_matching: all_matching.get_untracked(),
            filters: applied_filters.get_untracked(),
            contact_ids: selected_ids.get_untracked(),
            excluded_contact_ids: excluded_ids.get_untracked(),
        };
        task_poll_generation.update(|generation| *generation += 1);
        busy.set(true);
        error.set(String::new());
        notice.set(String::new());
        spawn_local(async move {
            // Re-check server-side: a contact may have been flagged since this
            // page listed them, so the visible selection can be stale.
            let blocked = match preview_blocked_recipients(selection.clone()).await {
                Ok(blocked) => blocked,
                Err(server_error) => {
                    error.set(err_text(server_error));
                    busy.set(false);
                    return;
                }
            };
            blocked_recipients.set(blocked.clone());
            let sending = (count - blocked.len() as i64).max(0);
            if sending == 0 {
                error.set(
                    "Every selected contact is now marked do not contact or archived, so there is nobody to send to."
                        .to_string(),
                );
                busy.set(false);
                return;
            }
            let mut confirmation = String::new();
            if !blocked.is_empty() {
                let names = blocked
                    .iter()
                    .take(5)
                    .map(|entry| format!("  - {} ({})", entry.name, entry.email))
                    .collect::<Vec<_>>()
                    .join("\n");
                let overflow = if blocked.len() > 5 {
                    format!("\n  and {} more", blocked.len() - 5)
                } else {
                    String::new()
                };
                confirmation.push_str(&format!(
                    "WARNING: {} of your selected contact{} marked do not contact or archived and will NOT receive this email:\n{names}{overflow}\n\n",
                    blocked.len(),
                    if blocked.len() == 1 { " is" } else { "s are" },
                ));
            }
            confirmation.push_str(&format!(
                "Send this message to {sending} contact{}?",
                if sending == 1 { "" } else { "s" },
            ));
            if !confirm(&confirmation) {
                busy.set(false);
                return;
            }
            match start_contact_mail_task(selection, launch_subject, launch_body).await {
                Ok(started) => {
                    task.set(Some(started));
                    blocked_recipients.set(Vec::new());
                    notice.set("Mail task started. This page will update as it sends.".to_string());
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
        if !confirm("Cancel this mail task? Any batch already in flight cannot be recalled.") {
            return;
        }
        task_poll_generation.update(|generation| *generation += 1);
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match cancel_contact_mail_task(current.id).await {
                Ok(updated) => {
                    let is_cancelling = updated.status
                        == crate::server_fns::contact_mail::ContactMailTaskStatus::Cancelling;
                    task.set(Some(updated));
                    notice.set(if is_cancelling {
                        "Cancellation requested.".to_string()
                    } else {
                        "The mail task had already finished.".to_string()
                    });
                }
                Err(server_error) => error.set(err_text(server_error)),
            }
            busy.set(false);
        });
    });

    let candidate_rows = move || {
        if loading.get() {
            return view! { <p class="p-4 text-sm text-slate-500">"Loading eligible contacts…"</p> }
                .into_any();
        }
        if candidates.get().is_empty() {
            return view! { <p class="p-4 text-sm text-slate-500">"No eligible contacts match these filters."</p> }
                .into_any();
        }
        candidates
            .get()
            .into_iter()
            .map(|candidate| {
                let id = candidate.id.clone();
                let checked_id = id.clone();
                let name = candidate.name;
                let email = candidate.email;
                let organization = candidate.organization;
                let has_organization = !organization.is_empty();
                let badges = candidate
                    .types
                    .into_iter()
                    .map(|kind| view! {
                        <span class=format!("rounded-full px-2 py-0.5 text-[0.68rem] {}", kind.badge_classes())>
                            {kind.label()}
                        </span>
                    })
                    .collect_view();
                view! {
                    <label class="flex cursor-pointer items-start gap-3 border-b border-slate-800 px-4 py-3 last:border-b-0 hover:bg-slate-800/40">
                        <input
                            type="checkbox"
                            class="mt-1 h-4 w-4 accent-primary-500"
                            prop:checked=move || {
                                if all_matching.get() {
                                    !excluded_ids.get().contains(&checked_id)
                                } else {
                                    selected_ids.get().contains(&checked_id)
                                }
                            }
                            on:change=move |event| {
                                let checked = event_target_checked(&event);
                                if all_matching.get_untracked() {
                                    excluded_ids.update(|ids| {
                                        if checked {
                                            ids.retain(|excluded| excluded != &id);
                                        } else if !ids.contains(&id) {
                                            ids.push(id.clone());
                                        }
                                    });
                                } else {
                                    selected_ids.update(|ids| {
                                        if checked && !ids.contains(&id) {
                                            ids.push(id.clone());
                                        } else if !checked {
                                            ids.retain(|selected| selected != &id);
                                        }
                                    });
                                }
                            }
                        />
                        <span class="min-w-0 flex-1">
                            <span class="block text-sm font-medium text-slate-100">{name}</span>
                            <span class="block break-all text-xs text-slate-400">{email}</span>
                            <Show when=move || has_organization>
                                <span class="block text-xs text-slate-500">{organization.clone()}</span>
                            </Show>
                            <span class="mt-1 flex flex-wrap gap-1">{badges}</span>
                        </span>
                    </label>
                }
            })
            .collect_view()
            .into_any()
    };

    view! {
        <div class="space-y-5">
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div>
                    <p class="max-w-3xl text-sm text-slate-400">
                        "Choose eligible Contacts, write one message, and send it in hidden-recipient batches."
                    </p>
                    <p class="mt-1 text-xs text-slate-500">
                        "Archived contacts, do-not-contact entries, invalid addresses, and duplicate addresses are excluded."
                    </p>
                </div>
                <A href="/contacts" attr:class="rounded-full border border-slate-700 px-3 py-2 text-sm text-slate-300 hover:bg-slate-800">
                    "Back to Contacts"
                </A>
            </div>

            <Show when=move || !error.get().is_empty()>
                <p role="alert" class="rounded-lg border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-sm text-rose-300">
                    {move || error.get()}
                </p>
            </Show>
            <Show when=move || !poll_error.get().is_empty()>
                <p role="alert" class="rounded-lg border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-sm text-rose-300">
                    {move || poll_error.get()}
                </p>
            </Show>
            <Show when=move || !notice.get().is_empty()>
                <p role="status" class="rounded-lg border border-emerald-500/40 bg-emerald-500/10 px-3 py-2 text-sm text-emerald-300">
                    {move || notice.get()}
                </p>
            </Show>

            <Show when=move || task.get().is_some()>
                <TaskProgress task cancel=cancel_task busy />
            </Show>

            <Show when=move || active.get()>
                <p class="rounded-xl border border-amber-500/30 bg-amber-500/10 p-4 text-sm text-amber-200">
                    "A contact mail task is active. New messages are blocked until it completes or cancellation finishes."
                </p>
            </Show>

            <fieldset disabled=move || active.get() || busy.get() class="space-y-5 disabled:opacity-70">
                <section class=PANEL>
                    <div class="flex flex-wrap items-start justify-between gap-3">
                        <div>
                            <h2 class="text-lg font-semibold text-slate-100">"1. Choose recipients"</h2>
                            <p class="mt-1 text-sm text-slate-500">"Filters match every selected category and the chosen contact type."</p>
                        </div>
                        <span class="rounded-full bg-primary-500/15 px-3 py-1 text-sm font-semibold text-primary-300">
                            {move || selected_count.get()} " selected"
                        </span>
                    </div>
                    <div class="mt-4 grid gap-3 md:grid-cols-3">
                        <label>
                            <span class=LABEL>"Search"</span>
                            <input class=INPUT placeholder="Name, organization, or email" prop:value=move || query.get()
                                on:input=move |event| query.set(event_target_value(&event)) />
                        </label>
                        <label>
                            <span class=LABEL>"Contact type"</span>
                            <select class=INPUT prop:value=move || contact_type.get()
                                on:change=move |event| contact_type.set(event_target_value(&event))>
                                <option value="">"Any type"</option>
                                {ContactType::ALL.iter().map(|kind| view! {
                                    <option value=kind.slug()>{kind.label()}</option>
                                }).collect_view()}
                            </select>
                        </label>
                        <label>
                            <span class=LABEL>"Organization"</span>
                            <select class=INPUT prop:value=move || organization_id.get()
                                on:change=move |event| organization_id.set(event_target_value(&event))>
                                <option value="">"Any organization"</option>
                                {move || organizations.get().into_iter().map(|(id, name)| view! {
                                    <option value=id>{name}</option>
                                }).collect_view()}
                            </select>
                        </label>
                    </div>
                    <details class="mt-3 rounded-lg border border-slate-800 bg-slate-950">
                        <summary class="cursor-pointer px-3 py-2 text-sm text-slate-300">"Categories and tags"</summary>
                        <div class="border-t border-slate-800 p-2">
                            <CategoryPicker
                                categories=Signal::derive(move || categories.get())
                                selected=category_ids
                            />
                        </div>
                    </details>
                    <button type="button" on:click=apply_filters
                        class="mt-3 rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50">
                        "Apply filters"
                    </button>

                    <div class="mt-4">
                        <PropertyFilterBar
                            subject=PropertySubject::Contact
                            filters=property_filters
                            scope=facet_scope
                        />
                    </div>

                    <div class="mt-5 flex flex-wrap items-center justify-between gap-2 text-xs text-slate-500">
                        <span>"Showing " {move || candidates.get().len()} " of " {move || total.get()} " eligible contacts"</span>
                        <div class="flex flex-wrap gap-2">
                            <button type="button" on:click=select_visible prop:disabled=move || loading.get()
                                class="text-primary-300 hover:text-primary-200 disabled:cursor-not-allowed disabled:opacity-40">"Select visible"</button>
                            <button type="button" on:click=move |_| {
                                let ids = candidates.get_untracked().into_iter().map(|candidate| candidate.id).collect::<Vec<_>>();
                                if all_matching.get_untracked() {
                                    excluded_ids.update(|excluded| {
                                        for id in ids {
                                            if !excluded.contains(&id) {
                                                excluded.push(id);
                                            }
                                        }
                                    });
                                } else {
                                    selected_ids.update(|selected| selected.retain(|id| !ids.contains(id)));
                                }
                            } prop:disabled=move || loading.get()
                                class="text-slate-300 hover:text-white disabled:cursor-not-allowed disabled:opacity-40">"Clear visible"</button>
                            <button type="button" on:click=move |_| {
                                all_matching.set(true);
                                selected_ids.set(Vec::new());
                                excluded_ids.set(Vec::new());
                            } prop:disabled=move || loading.get()
                                class="text-primary-300 hover:text-primary-200 disabled:cursor-not-allowed disabled:opacity-40">
                                "Select all matching"
                            </button>
                            <button type="button" on:click=clear_selection class="text-slate-300 hover:text-white">"Clear selection"</button>
                        </div>
                    </div>
                    <Show when=move || all_matching.get()>
                        <p class="mt-2 text-xs text-primary-300">
                            "All eligible matching contacts are selected except " {move || excluded_ids.get().len()} "."
                        </p>
                    </Show>
                    <div class="mt-3 overflow-hidden rounded-lg border border-slate-800 bg-slate-950">
                        {candidate_rows}
                    </div>
                    <div class="mt-3 flex justify-between gap-3">
                        <button type="button" on:click=move |_| offset.update(|value| *value = (*value - 50).max(0))
                            prop:disabled=move || offset.get() == 0
                            class="rounded-full border border-slate-700 px-3 py-1.5 text-sm text-slate-300 disabled:opacity-40">"Previous"</button>
                        <button type="button" on:click=move |_| offset.update(|value| *value += 50)
                            prop:disabled=move || {
                                offset.get() + candidates.get().len() as i64 >= total.get()
                            }
                            class="rounded-full border border-slate-700 px-3 py-1.5 text-sm text-slate-300 disabled:opacity-40">"Next"</button>
                    </div>
                </section>

                <section class=PANEL>
                    <h2 class="text-lg font-semibold text-slate-100">"2. Write the message"</h2>
                    <p class="mt-1 text-sm text-slate-500">"Plain text is safely formatted in the branded email template."</p>
                    <label class="mt-4 block">
                        <span class=LABEL>"Subject"</span>
                        <input class=INPUT maxlength=MAX_MAIL_SUBJECT prop:value=move || subject.get()
                            on:input=move |event| subject.set(event_target_value(&event)) />
                        <span class="mt-1 block text-right text-xs text-slate-600">
                            {move || subject.get().chars().count()} "/" {MAX_MAIL_SUBJECT}
                        </span>
                    </label>
                    <label class="mt-3 block">
                        <span class=LABEL>"Message"</span>
                        <textarea class=INPUT rows="10" maxlength=MAX_MAIL_BODY prop:value=move || body.get()
                            on:input=move |event| body.set(event_target_value(&event))></textarea>
                        <span class="mt-1 block text-right text-xs text-slate-600">
                            {move || body.get().chars().count()} "/" {MAX_MAIL_BODY}
                        </span>
                    </label>
                </section>

                <section class=PANEL>
                    <h2 class="text-lg font-semibold text-slate-100">"3. Review and start"</h2>
                    <Show when=move || !blocked_recipients.get().is_empty()>
                        <div role="alert" class="mt-3 rounded-lg border border-amber-500/40 bg-amber-500/10 p-3">
                            <p class="text-sm font-semibold text-amber-200">
                                {move || {
                                    let count = blocked_recipients.get().len();
                                    format!(
                                        "{count} selected contact{} marked do not contact or archived and will not be emailed.",
                                        if count == 1 { " is" } else { "s are" },
                                    )
                                }}
                            </p>
                            <ul class="mt-2 space-y-1">
                                {move || blocked_recipients.get().into_iter().map(|entry| {
                                    let reason = if entry.do_not_contact { "Do not contact" } else { "Archived" };
                                    view! {
                                        <li class="text-xs text-amber-200">
                                            {entry.name} " - " {entry.email} " - " <span class="font-medium">{reason}</span>
                                        </li>
                                    }
                                }).collect_view()}
                            </ul>
                            <p class="mt-2 text-xs text-amber-200">
                                "Sending skips them automatically; clear them from the selection to remove this notice."
                            </p>
                        </div>
                    </Show>
                    <p class="mt-2 text-sm text-slate-300">
                        "This will send to " <strong>{move || selected_count.get()}</strong> " contact"
                        {move || if selected_count.get() == 1 { "" } else { "s" }}
                        " in hidden-recipient batches."
                    </p>
                    <button type="button" on:click=launch
                        prop:disabled=move || busy.get() || selected_count.get() == 0 || subject.get().trim().is_empty() || body.get().trim().is_empty()
                        class="mt-4 rounded-full bg-primary-500 px-5 py-2.5 text-sm font-semibold text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:opacity-50">
                        {move || if busy.get() { "Starting…" } else { "Start mail task" }}
                    </button>
                </section>
            </fieldset>
        </div>
    }
}

#[component]
fn TaskProgress(
    task: RwSignal<Option<ContactMailTask>>,
    cancel: Callback<()>,
    busy: RwSignal<bool>,
) -> impl IntoView {
    move || {
        let Some(current) = task.get() else {
            return ().into_any();
        };
        let processed = current.processed_count();
        let remaining = current.remaining_count();
        let percent = if current.recipient_total == 0 {
            0
        } else {
            (processed * 100 / current.recipient_total).clamp(0, 100)
        };
        let status = current.status;
        let remaining_label =
            if status == crate::server_fns::contact_mail::ContactMailTaskStatus::Cancelled {
                "Not attempted"
            } else {
                "Remaining"
            };
        let status_class = match status {
            crate::server_fns::contact_mail::ContactMailTaskStatus::Completed => {
                "bg-emerald-500/15 text-emerald-300"
            }
            crate::server_fns::contact_mail::ContactMailTaskStatus::Cancelled => {
                "bg-slate-700 text-slate-200"
            }
            crate::server_fns::contact_mail::ContactMailTaskStatus::Failed => {
                "bg-rose-500/15 text-rose-300"
            }
            crate::server_fns::contact_mail::ContactMailTaskStatus::Cancelling => {
                "bg-amber-500/15 text-amber-300"
            }
            _ => "bg-primary-500/15 text-primary-300",
        };
        let failures = current.failures.clone();
        let completed_at = current.completed_at.clone();
        let cancel_requested_at = current.cancel_requested_at.clone();
        let cancel_requested_by_name = current.cancel_requested_by_name.clone();
        let task_error = current.error.clone();
        view! {
            <section class=PANEL>
                <div class="flex flex-wrap items-start justify-between gap-3">
                    <div>
                        <div class="flex flex-wrap items-center gap-2">
                            <h2 class="text-lg font-semibold text-slate-100">"Mail task"</h2>
                            <span class=format!("rounded-full px-2.5 py-1 text-xs font-semibold {status_class}")>{status.label()}</span>
                        </div>
                        <p class="mt-1 text-sm font-medium text-slate-300">{current.subject.clone()}</p>
                        <p class="mt-1 text-xs text-slate-500">"Created by " {current.created_by_name.clone()} " on " {current.created_at.clone()}</p>
                    </div>
                    <Show when=move || status.is_active()>
                        <button type="button" on:click=move |_| cancel.run(()) prop:disabled=move || busy.get() || status == crate::server_fns::contact_mail::ContactMailTaskStatus::Cancelling
                            class="rounded-full border border-rose-500/50 px-3 py-2 text-sm font-medium text-rose-300 hover:bg-rose-500/10 disabled:opacity-50">
                            {if status == crate::server_fns::contact_mail::ContactMailTaskStatus::Cancelling { "Cancelling…" } else { "Cancel task" }}
                        </button>
                    </Show>
                </div>
                <div class="mt-5" role="progressbar" aria-label="Mail task progress"
                    aria-valuemin="0" aria-valuemax=current.recipient_total aria-valuenow=processed>
                    <div class="h-3 overflow-hidden rounded-full bg-slate-800">
                        <div class="h-full rounded-full bg-primary-500 transition-all" style=format!("width: {percent}%")></div>
                    </div>
                    <p class="mt-2 text-sm text-slate-300">{processed} " of " {current.recipient_total} " recipients processed (" {percent} "%)"</p>
                </div>
                <dl class="mt-4 grid gap-3 text-sm sm:grid-cols-3">
                    <div class="rounded-lg bg-slate-950 p-3"><dt class="text-xs text-slate-500">"Provider accepted"</dt><dd class="mt-1 text-lg font-semibold text-emerald-300">{current.accepted_count}</dd></div>
                    <div class="rounded-lg bg-slate-950 p-3"><dt class="text-xs text-slate-500">"Failed attempts"</dt><dd class="mt-1 text-lg font-semibold text-rose-300">{current.failed_count}</dd></div>
                    <div class="rounded-lg bg-slate-950 p-3"><dt class="text-xs text-slate-500">{remaining_label}</dt><dd class="mt-1 text-lg font-semibold text-slate-100">{remaining}</dd></div>
                </dl>
                {if !completed_at.is_empty() {
                    view! { <p class="mt-3 text-xs text-slate-500">"Finished: " {completed_at}</p> }.into_any()
                } else { ().into_any() }}
                {if !cancel_requested_at.is_empty() {
                    view! { <p class="mt-2 text-xs text-amber-300">"Cancellation requested by " {cancel_requested_by_name} " on " {cancel_requested_at}</p> }.into_any()
                } else { ().into_any() }}
                {if !task_error.is_empty() {
                    view! { <p class="mt-3 text-sm text-rose-300" role="alert">{task_error}</p> }.into_any()
                } else { ().into_any() }}
                {if failures.is_empty() {
                    ().into_any()
                } else {
                    let failure_count = failures.len();
                    view! {
                        <details class="mt-4 rounded-lg border border-rose-500/20 bg-rose-500/5">
                            <summary class="cursor-pointer px-3 py-2 text-sm font-medium text-rose-300">"Failed recipients (" {failure_count} ")"</summary>
                            <div class="border-t border-rose-500/20">
                                {failures.into_iter().map(|failure| view! {
                                    <div class="border-b border-rose-500/10 px-3 py-2 text-xs last:border-b-0">
                                        <p class="text-slate-200">{failure.name} " · " {failure.email}</p>
                                        <p class="mt-1 text-rose-300">{failure.error}</p>
                                    </div>
                                }).collect_view()}
                            </div>
                        </details>
                    }.into_any()
                }}
            </section>
        }
        .into_any()
    }
}

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
