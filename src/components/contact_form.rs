//! The create/edit form for a contact, shared by the directory and the person
//! page so both write a person the same way.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::server_fns::contacts::{
    create_contact, update_contact, Contact, ContactInput, ContactType, MAX_TYPES,
};
use crate::server_fns::err_text;
use crate::server_fns::organizations::{search_active_organizations, ActiveOrganizationSummary};

const INPUT: &str =
    "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-600 focus:border-primary-500 focus:outline-none";
const LABEL: &str = "text-xs font-medium text-slate-400";

/// The fixed broad roles shared by every contact create and edit workflow.
#[component]
pub fn ContactTypeSelector(
    selected: Signal<Vec<ContactType>>,
    on_toggle: Callback<ContactType>,
) -> impl IntoView {
    view! {
        <fieldset>
            <legend class=LABEL>"Contact types *"</legend>
            <p class="mt-1 text-xs text-slate-500">
                "Choose 1 to 6 broad roles. Types are required and fixed; categories and tags are optional, organization-defined groupings."
            </p>
            <div class="mt-2 flex flex-wrap gap-2">
                {ContactType::ALL
                    .iter()
                    .map(|contact_type| {
                        let contact_type = *contact_type;
                        let is_selected = move || selected.get().contains(&contact_type);
                        let at_limit = move || {
                            !is_selected() && selected.get().len() >= MAX_TYPES
                        };
                        view! {
                            <button
                                type="button"
                                on:click=move |_| on_toggle.run(contact_type)
                                aria-pressed=move || is_selected().to_string()
                                prop:disabled=at_limit
                                class=move || {
                                    if is_selected() {
                                        "rounded-full border border-primary-500/40 bg-primary-500/15 px-3 py-1 text-xs font-medium text-primary-200"
                                    } else {
                                        "rounded-full border border-slate-700 px-3 py-1 text-xs font-medium text-slate-400 hover:bg-slate-800 disabled:cursor-not-allowed disabled:opacity-40"
                                    }
                                }
                            >
                                {contact_type.label()}
                            </button>
                        }
                    })
                    .collect_view()}
            </div>
            <p class="mt-2 text-xs text-slate-500" aria-live="polite">
                {move || format!("{} of {MAX_TYPES} selected", selected.get().len())}
            </p>
        </fieldset>
    }
}

/// Edits `contact` when given, otherwise creates a new person. `on_saved`
/// receives the contact id so the caller can close the form and refresh.
#[component]
pub fn ContactForm(
    contact: Option<Contact>,
    organizations: RwSignal<Vec<(String, String)>>,
    #[prop(optional)] initial_organization_id: String,
    on_saved: Callback<String>,
) -> impl IntoView {
    let existing_id = contact.as_ref().map(|c| c.id.clone());
    let editing = StoredValue::new(existing_id.clone());

    let seed = contact.unwrap_or_default();
    // REQ-CRM-047: linked-account identity is projected from `users` and cannot
    // be edited through the independent CRM contact form.
    let account_owned_identity = seed.has_account();
    let initial_organization_name = if !seed.organization_name.is_empty() {
        seed.organization_name.clone()
    } else {
        organizations
            .get_untracked()
            .into_iter()
            .find_map(|(id, name)| (id == initial_organization_id).then_some(name))
            .unwrap_or_default()
    };
    let first_name = RwSignal::new(seed.first_name.clone());
    let last_name = RwSignal::new(seed.last_name.clone());
    let preferred_name = RwSignal::new(seed.preferred_name.clone());
    let email = RwSignal::new(seed.email.clone());
    let phone = RwSignal::new(seed.phone.clone());
    let mobile = RwSignal::new(seed.mobile.clone());
    let address = RwSignal::new(seed.address.clone());
    let job_title = RwSignal::new(seed.job_title.clone());
    let organization_id = RwSignal::new(if seed.organization_id.is_empty() {
        initial_organization_id
    } else {
        seed.organization_id.clone()
    });
    let organization_query = RwSignal::new(initial_organization_name);
    let debounced_organization_query = RwSignal::new(String::new());
    let organization_results = RwSignal::new(Vec::<ActiveOrganizationSummary>::new());
    let organization_picker_open = RwSignal::new(false);
    let organization_search_generation = RwSignal::new(0u64);
    let selected_organization_archived = seed.organization_archived;
    let source = RwSignal::new(seed.source.clone());
    let description = RwSignal::new(seed.description.clone());
    let do_not_contact = RwSignal::new(seed.do_not_contact);
    let types = RwSignal::new(seed.types.clone());

    let error = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    Effect::new(move |_| {
        if !organization_picker_open.get() {
            return;
        }
        let query = debounced_organization_query.get();
        organization_search_generation.update(|generation| *generation += 1);
        let generation = organization_search_generation.get_untracked();
        spawn_local(async move {
            if let Ok(list) = search_active_organizations(query).await {
                if organization_search_generation.get_untracked() == generation {
                    organization_results.set(list);
                }
            }
        });
    });

    let submit = move |_| {
        if busy.get_untracked() {
            return;
        }
        let input = ContactInput {
            first_name: first_name.get_untracked(),
            last_name: last_name.get_untracked(),
            preferred_name: preferred_name.get_untracked(),
            email: email.get_untracked(),
            phone: phone.get_untracked(),
            mobile: mobile.get_untracked(),
            address: address.get_untracked(),
            job_title: job_title.get_untracked(),
            organization_id: organization_id.get_untracked(),
            types: types.get_untracked(),
            source: source.get_untracked(),
            description: description.get_untracked(),
            do_not_contact: do_not_contact.get_untracked(),
        };
        // Fail locally first so an obvious mistake does not need a round trip.
        if let Err(message) = input.validate() {
            error.set(message);
            return;
        }
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = match editing.get_value() {
                Some(id) => update_contact(id.clone(), input).await.map(|()| id),
                None => create_contact(input).await,
            };
            match result {
                Ok(id) => on_saved.run(id),
                Err(e) => error.set(err_text(e)),
            }
            busy.set(false);
        });
    };

    let text_field =
        move |label: &'static str, signal: RwSignal<String>, placeholder: &'static str| {
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

    let mut on_organization_search = debounce(
        std::time::Duration::from_millis(300),
        move |value: String| debounced_organization_query.set(value),
    );

    view! {
        <div class="space-y-4">
            <div class="grid gap-3 sm:grid-cols-2">
                <IdentityField label="Account first name" signal=first_name locked=account_owned_identity />
                <IdentityField label="Account last name" signal=last_name locked=account_owned_identity />
                {text_field("Preferred name", preferred_name, "")}
                {text_field("Job title", job_title, "")}
                <IdentityField label="Sign-in email" signal=email locked=account_owned_identity />
                <IdentityField label="Account phone" signal=phone locked=account_owned_identity />
                {text_field("Mobile", mobile, "")}
                {text_field("Source", source, "How we met them")}
            </div>

            <div class="relative">
                <label class="block">
                    <span class=LABEL>"Organization"</span>
                    <input
                        type="search"
                        role="combobox"
                        aria-autocomplete="list"
                        aria-expanded=move || organization_picker_open.get().to_string()
                        class=INPUT
                        placeholder="Search active organizations"
                        prop:value=move || organization_query.get()
                        on:focus=move |_| organization_picker_open.set(true)
                        on:input=move |event| {
                            organization_id.set(String::new());
                            let value = event_target_value(&event);
                            organization_query.set(value.clone());
                            on_organization_search(value);
                            organization_picker_open.set(true);
                        }
                        on:keydown=move |event: leptos::ev::KeyboardEvent| {
                            if event.key() == "Escape" {
                                organization_picker_open.set(false);
                            }
                        }
                    />
                </label>
                <Show when=move || organization_picker_open.get()>
                    <div role="listbox" class="absolute z-10 mt-1 max-h-52 w-full overflow-y-auto rounded-xl border border-slate-700 bg-slate-900">
                        <button
                            type="button"
                            class="block w-full px-3 py-2 text-left text-sm text-slate-400 hover:bg-slate-800"
                            on:click=move |_| {
                                organization_id.set(String::new());
                                organization_query.set(String::new());
                                organization_picker_open.set(false);
                            }
                        >
                            "No organization"
                        </button>
                        {move || organization_results.get().into_iter().map(|organization| {
                            let id = organization.id.clone();
                            let name = organization.name.clone();
                            view! {
                                <button
                                    type="button"
                                    role="option"
                                    class="block w-full px-3 py-2 text-left text-sm text-slate-200 hover:bg-slate-800"
                                    on:click=move |_| {
                                        organization_id.set(id.clone());
                                        organization_query.set(name.clone());
                                        organization_picker_open.set(false);
                                    }
                                >
                                    {organization.name} " · " {organization.kind.label()}
                                </button>
                            }
                        }).collect_view()}
                    </div>
                </Show>
                <Show when=move || selected_organization_archived && !organization_id.get().is_empty()>
                    <p class="mt-1 text-xs text-amber-300">"This existing organization is archived. It can remain, but cannot be selected for a new link."</p>
                </Show>
            </div>

            <IdentityField label="Account address" signal=address locked=account_owned_identity />
            <Show when=move || account_owned_identity>
                <p class="text-xs text-slate-500">
                    "Account identity fields are read-only here. Change them on the linked profile; preferred name, mobile, job title, and CRM fields remain editable."
                </p>
            </Show>

            <ContactTypeSelector
                selected=Signal::derive(move || types.get())
                on_toggle=Callback::new(move |contact_type| {
                    types.update(|items| {
                        if let Some(index) = items.iter().position(|item| *item == contact_type) {
                            items.remove(index);
                        } else if items.len() < MAX_TYPES {
                            items.push(contact_type);
                        }
                    });
                })
            />

            <label class="block">
                <span class=LABEL>"Notes"</span>
                <textarea
                    class=INPUT
                    rows="3"
                    prop:value=move || description.get()
                    on:input=move |e| description.set(event_target_value(&e))
                />
            </label>

            <label class="flex items-center gap-2 text-sm text-slate-300">
                <input
                    type="checkbox"
                    class="h-4 w-4 rounded border-slate-700 bg-slate-950"
                    prop:checked=move || do_not_contact.get()
                    on:change=move |e| do_not_contact.set(event_target_checked(&e))
                />
                "Do not contact"
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
                        "Create contact"
                    }
                }}
            </button>
        </div>
    }
}

#[component]
fn IdentityField(label: &'static str, signal: RwSignal<String>, locked: bool) -> impl IntoView {
    view! {
        <label class="block">
            <span class=LABEL>{label}</span>
            <input
                class=INPUT
                prop:value=move || signal.get()
                readonly=locked
                aria-readonly=locked.to_string()
                on:input=move |event| {
                    if !locked {
                        signal.set(event_target_value(&event));
                    }
                }
            />
        </label>
    }
}
