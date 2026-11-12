//! The structured detail view behind the canonical `/contacts/:id` route.
//!
//! The surrounding Contacts page owns routing and the information-access guard;
//! this module keeps the role-sensitive structured fields and connected records.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;

use crate::components::change_log::ChangeLog;
use crate::components::contact_cases::ContactCasesPanel;
use crate::components::contact_form::ContactForm;
use crate::components::contact_properties::ContactPropertiesPanel;
use crate::components::loading::Loading;
use crate::helpers::format::badge_pill;
use crate::pages::contacts::ContactOutreachPanel;
use crate::server_fns::audit::AuditScope;
use crate::server_fns::contacts::{
    load_contact, set_contact_account, set_contact_archived, unlinked_accounts, Contact,
};
use crate::server_fns::err_text;
use crate::server_fns::organizations::{list_organizations, OrganizationFilters};
use crate::state::AppState;

const PANEL: &str = "rounded-card border border-slate-800 bg-slate-900 shadow-card p-5";
const INPUT: &str =
    "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-600 focus:border-primary-500 focus:outline-none";

#[component]
pub fn ContactDetail(contact_id: String) -> impl IntoView {
    let state = expect_context::<AppState>();
    let can_manage = state.has_information_management_access();
    let can_manage_accounts = state.has_operations_admin_permissions();
    let id = StoredValue::new(contact_id);

    let contact = RwSignal::new(None::<Contact>);
    let loading = RwSignal::new(true);
    let error = RwSignal::new(String::new());
    let editing = RwSignal::new(false);
    let reload = RwSignal::new(0u32);
    let organizations = RwSignal::new(Vec::<(String, String)>::new());

    Effect::new(move |_| {
        spawn_local(async move {
            if let Ok(page) = list_organizations(OrganizationFilters::default(), 0, 200).await {
                organizations.set(page.items.into_iter().map(|o| (o.id, o.name)).collect());
            }
        });
    });

    Effect::new(move |_| {
        reload.track();
        loading.set(true);
        spawn_local(async move {
            match load_contact(id.get_value()).await {
                Ok(found) => {
                    contact.set(found);
                    error.set(String::new());
                }
                Err(e) => error.set(err_text(e)),
            }
            loading.set(false);
        });
    });

    let toggle_archive = move |_| {
        let Some(current) = contact.get_untracked() else {
            return;
        };
        let next = !current.archived;
        spawn_local(async move {
            match set_contact_archived(id.get_value(), next).await {
                Ok(()) => reload.update(|r| *r += 1),
                Err(e) => error.set(err_text(e)),
            }
        });
    };

    view! {
        <div class="space-y-6">
            <Show when=move || loading.get() && contact.get().is_none()>
                <div class=PANEL><Loading label="Loading person\u{2026}" /></div>
            </Show>
            <Show when=move || !error.get().is_empty()>
                <p class="text-sm text-rose-300">{move || error.get()}</p>
            </Show>

            {move || {
                let Some(person) = contact.get() else {
                    return ().into_any();
                };
                let name = person.display_name();
                let archived = person.archived;
                let do_not_contact = person.do_not_contact;
                let types = person.types.clone();
                let editable = person.clone();
                let linked = person.clone();
                view! {
                    <div class=PANEL>
                        <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                            <div class="min-w-0">
                                <div class="flex flex-wrap items-center gap-2">
                                    <h2 class="text-lg font-semibold text-slate-100">{name}</h2>
                                    <Show when=move || archived>
                                        <span class=badge_pill("bg-slate-700/40 text-slate-300 ring-1 ring-slate-600")>"Archived"</span>
                                    </Show>
                                    <Show when=move || do_not_contact>
                                        <span class=badge_pill("bg-rose-500/15 text-rose-300 ring-1 ring-rose-500/30")>"Do not contact"</span>
                                    </Show>
                                    {types
                                        .iter()
                                        .map(|t| view! {
                                            <span class=badge_pill(t.badge_classes())>{t.label()}</span>
                                        })
                                        .collect_view()}
                                </div>
                            </div>
                            <Show when=move || can_manage>
                                <div class="flex shrink-0 gap-2">
                                    <button
                                        type="button"
                                        on:click=move |_| editing.update(|e| *e = !*e)
                                        class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800"
                                    >
                                        {move || if editing.get() { "Cancel" } else { "Edit" }}
                                    </button>
                                    <button
                                        type="button"
                                        on:click=toggle_archive
                                        class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-300 hover:bg-slate-800"
                                    >
                                        {if archived { "Restore" } else { "Archive" }}
                                    </button>
                                </div>
                            </Show>
                        </div>

                        <Show
                            when=move || editing.get()
                            fallback=move || view! { <ContactSummary contact=editable.clone() /> }
                        >
                            <div class="mt-4 border-t border-slate-800 pt-4">
                                <ContactForm
                                    contact=contact.get()
                                    organizations=organizations
                                    on_saved=Callback::new(move |_id: String| {
                                        editing.set(false);
                                        reload.update(|r| *r += 1);
                                    })
                                />
                            </div>
                        </Show>
                    </div>

                    <Show when=move || can_manage>
                        <ContactPropertiesPanel contact_id=id.get_value() />
                    </Show>

                    <ContactOutreachPanel
                        contact_id=id.get_value()
                        contact_changed=Callback::new(move |()| reload.update(|value| *value += 1))
                    />

                    <Show when=move || person.has_account_field_conflict>
                        <div class="rounded-xl border border-amber-500/30 bg-amber-500/10 p-4 text-sm text-amber-200">
                            "This linked person has preserved CRM identity values that differ from the account. The account-owned values shown above are authoritative; review the migration conflict record before unlinking."
                        </div>
                    </Show>

                    <Show when=move || can_manage>
                        <ContactCasesPanel contact_id=id.get_value() />
                    </Show>

                    <Show when=move || can_manage_accounts>
                        <AccountLink
                            contact=linked.clone()
                            can_manage=true
                            on_changed=Callback::new(move |_: ()| reload.update(|r| *r += 1))
                        />
                    </Show>

                    <Show when=move || can_manage>
                        <div class=PANEL>
                            <h3 class="text-sm font-semibold text-slate-200">"Change log"</h3>
                            <div class="mt-3">
                                <ChangeLog scope=AuditScope::Contact entity_id=id.get_value() />
                            </div>
                        </div>
                    </Show>
                }
                .into_any()
            }}

            <A href="/contacts" attr:class="inline-block text-sm text-primary-400 hover:text-primary-300">
                "\u{2190} Back to contacts"
            </A>
        </div>
    }
}

/// The link between this person and a sign-in account.
///
/// A contact and a user are separate records on purpose (ADR-0005), so this
/// states plainly which account — if any — belongs to this person, and links
/// through to their profile. Linking and unlinking never touch the account
/// itself: unlinking leaves them able to sign in exactly as before.
#[component]
fn AccountLink(contact: Contact, can_manage: bool, on_changed: Callback<()>) -> impl IntoView {
    let contact_id = StoredValue::new(contact.id.clone());
    let has_account = contact.has_account();
    let user_id = contact.user_id.clone();
    let email = contact.linked_email.clone();
    let role = contact.linked_role;
    let profile_href = format!("/profile/{user_id}");

    let choices = RwSignal::new(Vec::<(String, String, String)>::new());
    let chosen = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let picking = RwSignal::new(false);

    // Accounts that have no person record yet, so linking cannot create a
    // duplicate. Only loaded when an admin actually opens the picker.
    Effect::new(move |_| {
        if !picking.get() || !choices.get_untracked().is_empty() {
            return;
        }
        spawn_local(async move {
            match unlinked_accounts().await {
                Ok(list) => {
                    choices.set(list.into_iter().map(|a| (a.id, a.name, a.email)).collect())
                }
                Err(e) => error.set(err_text(e)),
            }
        });
    });

    let apply = move |target: Option<String>| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match set_contact_account(contact_id.get_value(), target.unwrap_or_default()).await {
                Ok(()) => {
                    picking.set(false);
                    on_changed.run(());
                }
                Err(e) => error.set(err_text(e)),
            }
            busy.set(false);
        });
    };

    view! {
        <div class=PANEL>
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div>
                    <h3 class="text-sm font-semibold text-slate-200">"Sign-in account"</h3>
                    <p class="mt-1 text-xs text-slate-500">
                        "A person and an account are separate records. Most people never need a login."
                    </p>
                </div>
                <Show when=move || can_manage && has_account>
                    <button
                        type="button"
                        prop:disabled=move || busy.get()
                        on:click=move |_| apply(None)
                        class="rounded-full border border-slate-700 px-3 py-1.5 text-xs font-medium text-slate-300 hover:bg-slate-800 disabled:opacity-50"
                    >
                        "Unlink"
                    </button>
                </Show>
            </div>

            <Show when=move || !error.get().is_empty()>
                <p class="mt-3 text-sm text-rose-300" role="alert">{move || error.get()}</p>
            </Show>

            {if has_account {
                let badge = role
                    .map(|r| view! {
                        <span class=badge_pill(r.badge_classes())>{r.label()}</span>
                    }.into_any())
                    .unwrap_or_else(|| ().into_any());
                view! {
                    <div class="mt-3 rounded-lg border border-slate-800 bg-slate-950 p-3">
                        <div class="flex flex-wrap items-center gap-2">
                            <A
                                href=profile_href.clone()
                                attr:class="text-sm font-medium text-primary-300 hover:text-primary-200"
                            >
                                "Open their profile"
                            </A>
                            {badge}
                        </div>
                        <p class="mt-1 text-xs text-slate-500">
                            "Signs in as " {email.clone()}
                            ". Email and role belong to the account and are changed under Manage users."
                        </p>
                    </div>
                }
                .into_any()
            } else {
                view! {
                    <p class="mt-3 text-sm text-slate-500">
                        "No account. This person cannot sign in."
                    </p>
                }
                .into_any()
            }}

            <Show when=move || can_manage && !has_account>
                <div class="mt-3">
                    <Show
                        when=move || picking.get()
                        fallback=move || view! {
                            <button
                                type="button"
                                on:click=move |_| picking.set(true)
                                class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800"
                            >
                                "Link an existing account"
                            </button>
                        }
                    >
                        <div class="flex flex-wrap gap-2">
                            <select
                                class=INPUT
                                prop:value=move || chosen.get()
                                on:change=move |e| chosen.set(event_target_value(&e))
                            >
                                <option value="">"Choose an account\u{2026}"</option>
                                {move || choices
                                    .get()
                                    .into_iter()
                                    .map(|(id, name, mail)| view! {
                                        <option value=id>{format!("{name} \u{2014} {mail}")}</option>
                                    })
                                    .collect_view()}
                            </select>
                            <button
                                type="button"
                                prop:disabled=move || busy.get()
                                on:click=move |_| {
                                    let id = chosen.get_untracked();
                                    if id.is_empty() {
                                        error.set("Choose an account to link.".into());
                                    } else {
                                        apply(Some(id));
                                    }
                                }
                                class="rounded-full bg-primary-500 px-3 py-1.5 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                            >
                                "Link"
                            </button>
                            <button
                                type="button"
                                on:click=move |_| picking.set(false)
                                class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-300 hover:bg-slate-800"
                            >
                                "Cancel"
                            </button>
                        </div>
                        <p class="mt-2 text-xs text-slate-500">
                            "Only accounts that have no person record yet are listed."
                        </p>
                    </Show>
                </div>
            </Show>
        </div>
    }
}

/// The read-only view of a contact's fields, including the linked account.
#[component]
fn ContactSummary(contact: Contact) -> impl IntoView {
    let row = |label: &'static str, value: String| {
        let empty = value.trim().is_empty();
        view! {
            <div class="border-b border-slate-800 py-2 last:border-b-0">
                <dt class="text-xs font-medium text-slate-500">{label}</dt>
                <dd class=if empty {
                    "mt-1 text-sm italic text-slate-500"
                } else {
                    "mt-1 whitespace-pre-wrap text-sm text-slate-200"
                }>{if empty { "Not provided".to_string() } else { value }}</dd>
            </div>
        }
    };

    let account = if contact.linked_email.is_empty() {
        String::new()
    } else {
        let role = contact
            .linked_role
            .map(|r| r.label().to_string())
            .unwrap_or_default();
        format!("{} ({role})", contact.linked_email)
    };

    view! {
        <dl class="mt-4 grid gap-x-6 sm:grid-cols-2">
            {row("Full name", format!("{} {}", contact.first_name, contact.last_name).trim().to_string())}
            {row("Preferred name", contact.preferred_name.clone())}
            <div class="border-b border-slate-800 py-2 last:border-b-0">
                <dt class="text-xs font-medium text-slate-500">"Organization"</dt>
                <dd class="mt-1 text-sm text-slate-200">
                    {if contact.organization_id.is_empty() {
                        view! { <span class="italic text-slate-500">"Not provided"</span> }.into_any()
                    } else {
                        view! {
                            <A
                                href=format!("/organizations/{}", contact.organization_id)
                                attr:class="text-primary-300 hover:text-primary-200"
                            >
                                {contact.organization_name.clone()}
                            </A>
                        }
                        .into_any()
                    }}
                </dd>
            </div>
            {row("Job title", contact.job_title.clone())}
            {row("Email", contact.email.clone())}
            {row("Phone", contact.phone.clone())}
            {row("Mobile", contact.mobile.clone())}
            {row("Address", contact.address.clone())}
            {row("Source", contact.source.clone())}
            {(!account.is_empty()).then(|| row("Sign-in account", account))}
            {row("Notes", contact.description.clone())}
        </dl>
    }
}
