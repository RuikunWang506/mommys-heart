//! The "People on this case" panel.
//!
//! Rendered only for staff. Reads follow `ViewCase`; every mutation remains
//! anchored to the stored case's `EditCase` capability.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;

use crate::helpers::format::badge_pill;
use crate::server_fns::case_contacts::{
    add_case_contact, list_case_contacts, remove_case_contact, update_case_contact, CaseContact,
    CaseContactRole,
};
use crate::server_fns::contacts::{search_active_contacts, ActiveContactSummary};
use crate::server_fns::err_text;
use crate::state::AppState;

pub(crate) const PANEL: &str = "rounded-card border border-slate-800 bg-slate-900 shadow-card p-5";
pub(crate) const INPUT: &str =
    "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-600 focus:border-primary-500 focus:outline-none";
pub(crate) const LABEL: &str = "text-xs font-medium text-slate-400";

#[component]
pub(crate) fn CaseLinkFields(
    id_prefix: String,
    role: RwSignal<String>,
    note: RwSignal<String>,
    is_primary: RwSignal<bool>,
    disabled: RwSignal<bool>,
    #[prop(into, optional)] primary_label: String,
) -> impl IntoView {
    let primary_label = if primary_label.trim().is_empty() {
        "Primary contact for this case".to_string()
    } else {
        primary_label
    };
    let role_id = StoredValue::new(format!("{id_prefix}-role"));
    let note_id = StoredValue::new(format!("{id_prefix}-note"));
    let primary_id = StoredValue::new(format!("{id_prefix}-primary"));

    view! {
        <div class="space-y-3">
            <label class="block">
                <span class=LABEL>"Role on this case"</span>
                <select
                    id=role_id.get_value()
                    class=INPUT
                    prop:disabled=move || disabled.get()
                    prop:value=move || role.get()
                    on:change=move |event| role.set(event_target_value(&event))
                >
                    {CaseContactRole::ALL
                        .iter()
                        .map(|item| view! { <option value=item.slug()>{item.label()}</option> })
                        .collect_view()}
                </select>
            </label>
            <label class="block">
                <span class=LABEL>"Note"</span>
                <textarea
                    id=note_id.get_value()
                    rows="2"
                    class=INPUT
                    prop:disabled=move || disabled.get()
                    prop:value=move || note.get()
                    on:input=move |event| note.set(event_target_value(&event))
                ></textarea>
            </label>
            <label class="flex items-center gap-2 text-sm text-slate-300">
                <input
                    id=primary_id.get_value()
                    type="checkbox"
                    class="h-4 w-4 rounded border-slate-700 bg-slate-950"
                    prop:disabled=move || disabled.get()
                    prop:checked=move || is_primary.get()
                    on:change=move |event| is_primary.set(event_target_checked(&event))
                />
                {primary_label}
            </label>
        </div>
    }
}

#[component]
fn CaseContactRow(
    person: CaseContact,
    can_edit: bool,
    can_link_person: bool,
    on_changed: Callback<()>,
) -> impl IntoView {
    let row_id = StoredValue::new(person.id.clone());
    let name = person.contact_name.clone();
    let href = format!("/contacts/{}", person.contact_id);
    let meta = {
        let mut parts = vec![if person.organization_name.is_empty() {
            "No organization".to_string()
        } else {
            person.organization_name.clone()
        }];
        if !person.email.is_empty() {
            parts.push(person.email.clone());
        }
        if !person.phone.is_empty() {
            parts.push(person.phone.clone());
        }
        parts.join(" · ")
    };
    let note_text = person.note.clone();
    let has_note = !note_text.trim().is_empty();
    let role_badge = person.role;
    let archived = person.contact_archived;
    let primary = person.is_primary;

    let role = RwSignal::new(person.role.slug().to_string());
    let note = RwSignal::new(person.note.clone());
    let is_primary = RwSignal::new(person.is_primary);
    let editing = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(String::new());

    let seed_role = StoredValue::new(person.role.slug().to_string());
    let seed_note = StoredValue::new(person.note.clone());
    let seed_primary = person.is_primary;

    let begin_edit = move |_| {
        error.set(String::new());
        editing.set(true);
    };
    let cancel = move |_| {
        role.set(seed_role.get_value());
        note.set(seed_note.get_value());
        is_primary.set(seed_primary);
        error.set(String::new());
        editing.set(false);
    };
    let save = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(String::new());
        let on_changed = on_changed.clone();
        spawn_local(async move {
            let result = update_case_contact(
                row_id.get_value(),
                CaseContactRole::from_slug(&role.get_untracked()).unwrap_or_default(),
                note.get_untracked(),
                is_primary.get_untracked(),
            )
            .await;
            match result {
                Ok(()) => {
                    editing.set(false);
                    on_changed.run(());
                }
                Err(e) => error.set(err_text(e)),
            }
            busy.set(false);
        });
    };
    let remove = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(String::new());
        let on_changed = on_changed.clone();
        spawn_local(async move {
            match remove_case_contact(row_id.get_value()).await {
                Ok(()) => on_changed.run(()),
                Err(e) => error.set(err_text(e)),
            }
            busy.set(false);
        });
    };

    let edit_label = StoredValue::new(format!("Edit link for {name}"));
    let remove_label = StoredValue::new(format!("Remove {name} from this case"));

    view! {
        <div
            class="rounded-lg border border-slate-800 bg-slate-950 p-3"
            aria-busy=move || busy.get().to_string()
        >
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div class="min-w-0">
                    <div class="flex flex-wrap items-center gap-2">
                        {if can_link_person {
                            view! {
                                <A href=href attr:class="text-sm font-semibold text-slate-100 hover:text-primary-300">
                                    {name.clone()}
                                </A>
                            }
                                .into_any()
                        } else {
                            view! { <span class="text-sm font-semibold text-slate-100">{name.clone()}</span> }
                                .into_any()
                        }}
                        <span class=badge_pill(role_badge.badge_classes())>{role_badge.label()}</span>
                        <Show when=move || primary>
                            <span class=badge_pill("bg-primary-500/15 text-primary-300 ring-1 ring-primary-500/30")>
                                "Primary"
                            </span>
                        </Show>
                        <Show when=move || archived>
                            <span class=badge_pill("bg-slate-700/40 text-slate-300 ring-1 ring-slate-600")>
                                "Archived"
                            </span>
                        </Show>
                    </div>
                    <p class="mt-1 text-xs text-slate-500">{meta}</p>
                    <Show when=move || !editing.get() && has_note>
                        <p class="mt-1 whitespace-pre-wrap text-xs text-slate-400">{note_text.clone()}</p>
                    </Show>
                </div>
                <Show when=move || can_edit && !editing.get()>
                    <div class="flex shrink-0 flex-wrap gap-2">
                        <button
                            type="button"
                            on:click=begin_edit
                            attr:aria-label=move || edit_label.get_value()
                            class="rounded-full border border-slate-700 px-2 py-1 text-xs font-medium text-slate-300 hover:bg-slate-800"
                        >
                            "Edit"
                        </button>
                        <button
                            type="button"
                            on:click=remove.clone()
                            prop:disabled=move || busy.get()
                            attr:aria-label=move || remove_label.get_value()
                            class="rounded-full border border-slate-700 px-2 py-1 text-xs font-medium text-slate-400 hover:bg-slate-800 disabled:opacity-50"
                        >
                            "Remove"
                        </button>
                    </div>
                </Show>
            </div>

            <Show when=move || editing.get()>
                <div class="mt-3 space-y-3 border-t border-slate-800 pt-3">
                    <CaseLinkFields
                        id_prefix=format!("case-contact-{}", row_id.get_value())
                        role
                        note
                        is_primary
                        disabled=busy
                    />
                    <div class="flex flex-wrap gap-2">
                        <button
                            type="button"
                            on:click=save
                            prop:disabled=move || busy.get()
                            class="rounded-full bg-primary-500 px-3 py-1.5 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                        >
                            {move || if busy.get() { "Saving…" } else { "Save changes" }}
                        </button>
                        <button
                            type="button"
                            on:click=cancel
                            prop:disabled=move || busy.get()
                            class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-300 hover:bg-slate-800 disabled:opacity-50"
                        >
                            "Cancel"
                        </button>
                        <button
                            type="button"
                            on:click=remove
                            prop:disabled=move || busy.get()
                            attr:aria-label=move || remove_label.get_value()
                            class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-400 hover:bg-slate-800 disabled:opacity-50"
                        >
                            "Remove"
                        </button>
                    </div>
                </div>
            </Show>

            <Show when=move || !error.get().is_empty()>
                <p class="mt-3 text-sm text-rose-300" role="alert">{move || error.get()}</p>
            </Show>
        </div>
    }
}

#[component]
pub fn CaseContactsPanel(case_id: String, can_edit: bool) -> impl IntoView {
    let state = expect_context::<AppState>();
    let id = StoredValue::new(case_id.clone());
    let add_region_id = StoredValue::new(format!("case-contact-add-{case_id}"));
    let search_id = StoredValue::new(format!("case-contact-search-{case_id}"));
    let results_id = StoredValue::new(format!("case-contact-results-{case_id}"));
    let add_prefix = StoredValue::new(format!("case-contact-new-{case_id}"));

    let people = RwSignal::new(Vec::<CaseContact>::new());
    let loading = RwSignal::new(true);
    let error = RwSignal::new(String::new());
    let reload = RwSignal::new(0u32);

    let adding = RwSignal::new(false);
    let searching = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let search_generation = RwSignal::new(0u64);
    let picker_open = RwSignal::new(false);
    let active_result = RwSignal::new(None::<usize>);
    let search_query = RwSignal::new(String::new());
    let debounced_search_query = RwSignal::new(String::new());
    let debounce_generation = RwSignal::new(0u64);
    let search_results = RwSignal::new(Vec::<ActiveContactSummary>::new());
    let selected_contact_id = RwSignal::new(String::new());
    let selected_label = RwSignal::new(String::new());

    let role = RwSignal::new(CaseContactRole::default().slug().to_string());
    let note = RwSignal::new(String::new());
    let is_primary = RwSignal::new(false);

    Effect::new(move |_| {
        reload.track();
        loading.set(true);
        spawn_local(async move {
            match list_case_contacts(id.get_value()).await {
                Ok(list) => {
                    people.set(list);
                    error.set(String::new());
                }
                Err(e) => error.set(err_text(e)),
            }
            loading.set(false);
        });
    });

    Effect::new(move |_| {
        if !adding.get() || !picker_open.get() {
            searching.set(false);
            return;
        }
        let query = debounced_search_query.get().trim().to_string();
        if query.is_empty() {
            search_results.set(Vec::new());
            active_result.set(None);
            searching.set(false);
            return;
        }
        search_generation.update(|generation| *generation += 1);
        let generation = search_generation.get_untracked();
        searching.set(true);
        spawn_local(async move {
            let response = search_active_contacts(query).await;
            if search_generation.get_untracked() != generation {
                return;
            }
            match response {
                Ok(list) => {
                    search_results.set(list);
                    error.set(String::new());
                }
                Err(e) => {
                    search_results.set(Vec::new());
                    error.set(err_text(e));
                }
            }
            active_result.set(None);
            searching.set(false);
        });
    });

    let toggle_adding = move |_| {
        if busy.get_untracked() {
            return;
        }
        let opening = !adding.get_untracked();
        adding.set(opening);
        picker_open.set(opening);
        error.set(String::new());
        search_query.set(String::new());
        debounced_search_query.set(String::new());
        search_results.set(Vec::new());
        active_result.set(None);
        selected_contact_id.set(String::new());
        selected_label.set(String::new());
        role.set(CaseContactRole::default().slug().to_string());
        note.set(String::new());
        is_primary.set(false);
    };

    let submit = move |_| {
        if busy.get_untracked() {
            return;
        }
        let chosen = selected_contact_id.get_untracked();
        if chosen.is_empty() {
            error.set("Choose a person to add.".into());
            return;
        }
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = add_case_contact(
                id.get_value(),
                chosen,
                CaseContactRole::from_slug(&role.get_untracked()).unwrap_or_default(),
                note.get_untracked(),
                is_primary.get_untracked(),
            )
            .await;
            match result {
                Ok(()) => {
                    adding.set(false);
                    picker_open.set(false);
                    search_query.set(String::new());
                    debounced_search_query.set(String::new());
                    search_results.set(Vec::new());
                    active_result.set(None);
                    selected_contact_id.set(String::new());
                    selected_label.set(String::new());
                    role.set(CaseContactRole::default().slug().to_string());
                    note.set(String::new());
                    is_primary.set(false);
                    reload.update(|value| *value += 1);
                }
                Err(e) => error.set(err_text(e)),
            }
            busy.set(false);
        });
    };

    let search_result_list = move || {
        if !picker_open.get() {
            return ().into_any();
        }
        if search_query.get().trim().is_empty() {
            return view! {
                <div
                    id=results_id.get_value()
                    role="listbox"
                    class="absolute z-10 mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-xs text-slate-500"
                >
                    "Type a name or organization to search active people."
                </div>
            }
            .into_any();
        }
        if searching.get() {
            return view! {
                <div
                    id=results_id.get_value()
                    role="listbox"
                    class="absolute z-10 mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-xs text-slate-500"
                >
                    "Searching…"
                </div>
            }
            .into_any();
        }
        let items = search_results.get();
        if items.is_empty() {
            return view! {
                <div
                    id=results_id.get_value()
                    role="listbox"
                    class="absolute z-10 mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-xs text-slate-500"
                >
                    "No matching people."
                </div>
            }
            .into_any();
        }
        let rows = items
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                let option_id = format!("{}-{index}", results_id.get_value());
                let option_label = item.label.clone();
                let selected_id = item.id.clone();
                let select = move |_| {
                    selected_contact_id.set(selected_id.clone());
                    selected_label.set(option_label.clone());
                    picker_open.set(false);
                    active_result.set(None);
                };
                view! {
                    <button
                        id=option_id
                        type="button"
                        role="option"
                        tabindex="-1"
                        aria-selected=move || (active_result.get() == Some(index)).to_string()
                        on:click=select
                        class=move || if active_result.get() == Some(index) {
                            "block w-full truncate bg-slate-800 px-3 py-1.5 text-left text-sm text-slate-100"
                        } else {
                            "block w-full truncate px-3 py-1.5 text-left text-sm text-slate-200 hover:bg-slate-800"
                        }
                    >
                        {item.label}
                    </button>
                }
            })
            .collect_view();
        view! {
            <div
                id=results_id.get_value()
                role="listbox"
                class="absolute z-10 mt-1 max-h-48 w-full overflow-y-auto rounded-xl border border-slate-700 bg-slate-900"
            >
                {rows}
            </div>
        }
        .into_any()
    };

    let rows = move || {
        let list = people.get();
        if list.is_empty() {
            let message = if loading.get() {
                "Loading people…"
            } else {
                "Nobody has been linked to this case yet."
            };
            return view! { <p class="text-sm text-slate-500">{message}</p> }.into_any();
        }
        let can_link_person = state.has_information_management_access();
        list.into_iter()
            .map(|person| {
                view! {
                    <CaseContactRow
                        person
                        can_edit
                        can_link_person
                        on_changed=Callback::new(move |_: ()| reload.update(|value| *value += 1))
                    />
                }
            })
            .collect_view()
            .into_any()
    };

    Effect::new(move |_| {
        let value = search_query.get();
        debounce_generation.update(|generation| *generation += 1);
        let generation = debounce_generation.get_untracked();
        set_timeout(
            move || {
                if debounce_generation.try_get_untracked() == Some(generation) {
                    let _ = debounced_search_query.try_set(value);
                }
            },
            std::time::Duration::from_millis(300),
        );
    });

    view! {
        <div
            class=PANEL
            aria-busy=move || (loading.get() || searching.get() || busy.get()).to_string()
        >
            <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                <div>
                    <h2 class="text-lg font-semibold text-slate-100">"People on this case"</h2>
                    <p class="mt-1 text-sm text-slate-500">
                        "Attorneys, caseworkers, emergency contacts, and anyone else involved."
                    </p>
                </div>
                <Show when=move || can_edit>
                    <button
                        type="button"
                        on:click=toggle_adding
                        aria-expanded=move || adding.get().to_string()
                        aria-controls=add_region_id.get_value()
                        class="shrink-0 rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600"
                    >
                        {move || if adding.get() { "Cancel" } else { "+ Add person" }}
                    </button>
                </Show>
            </div>

            <Show when=move || adding.get()>
                <div id=add_region_id.get_value() class="mt-4 space-y-3 border-t border-slate-800 pt-4">
                    <div class="relative">
                        <label class="block">
                            <span class=LABEL>"Person"</span>
                            <input
                                id=search_id.get_value()
                                type="search"
                                role="combobox"
                                aria-autocomplete="list"
                                aria-expanded=move || picker_open.get().to_string()
                                aria-controls=results_id.get_value()
                                aria-activedescendant=move || {
                                    active_result
                                        .get()
                                        .map(|index| format!("{}-{index}", results_id.get_value()))
                                }
                                class=INPUT
                                placeholder="Search by name or organization"
                                prop:disabled=move || busy.get()
                                prop:value=move || {
                                    let label = selected_label.get();
                                    if label.is_empty() { search_query.get() } else { label }
                                }
                                on:focus=move |_| {
                                    picker_open.set(true);
                                    active_result.set(None);
                                }
                                on:input=move |event| {
                                    selected_contact_id.set(String::new());
                                    selected_label.set(String::new());
                                    search_query.set(event_target_value(&event));
                                    picker_open.set(true);
                                    active_result.set(None);
                                }
                                on:keydown=move |event: leptos::ev::KeyboardEvent| {
                                    let count = search_results.get_untracked().len();
                                    match event.key().as_str() {
                                        "ArrowDown" if count > 0 => {
                                            event.prevent_default();
                                            active_result.update(|active| {
                                                *active = Some(active.map_or(0, |index| (index + 1).min(count - 1)));
                                            });
                                        }
                                        "ArrowUp" if count > 0 => {
                                            event.prevent_default();
                                            active_result.update(|active| {
                                                *active = Some(active.map_or(count - 1, |index| index.saturating_sub(1)));
                                            });
                                        }
                                        "Enter" => {
                                            if let Some(index) = active_result.get_untracked() {
                                                event.prevent_default();
                                                if let Some(item) = search_results.get_untracked().get(index).cloned() {
                                                    selected_contact_id.set(item.id);
                                                    selected_label.set(item.label);
                                                    picker_open.set(false);
                                                    active_result.set(None);
                                                }
                                            }
                                        }
                                        "Escape" => {
                                            picker_open.set(false);
                                            active_result.set(None);
                                        }
                                        _ => {}
                                    }
                                }
                            />
                        </label>
                        {search_result_list}
                    </div>

                    <CaseLinkFields
                        id_prefix=add_prefix.get_value()
                        role
                        note
                        is_primary
                        disabled=busy
                    />

                    <p class="text-xs text-slate-500">
                        "Not finding them? Add the person under Contacts first."
                    </p>

                    <button
                        type="button"
                        on:click=submit
                        prop:disabled=move || busy.get() || selected_contact_id.get().is_empty()
                        class="rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                    >
                        {move || if busy.get() { "Adding…" } else { "Add to case" }}
                    </button>
                </div>
            </Show>

            <Show when=move || !error.get().is_empty()>
                <p class="mt-3 text-sm text-rose-300" role="alert">{move || error.get()}</p>
            </Show>

            <div class="mt-4 space-y-2">{rows}</div>
        </div>
    }
}
