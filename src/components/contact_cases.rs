//! The "Cases involving this person" panel.
//!
//! Rendered on the admin person detail view. Reads follow the admin-read path;
//! each mutation still resolves the stored case and requires `EditCase` there.

use crate::components::case_contacts::{CaseLinkFields, INPUT, LABEL, PANEL};
use crate::helpers::format::badge_pill;
use crate::server_fns::case_contacts::{
    add_case_contact, list_contact_cases, remove_case_contact, search_editable_cases,
    update_case_contact, CaseContactRole, ContactCaseLink, EditableCaseSummary,
};
use crate::server_fns::err_text;
use leptos::prelude::*;
use leptos::task::spawn_local;

#[component]
fn ContactCaseRow(link: ContactCaseLink, on_changed: Callback<()>) -> impl IntoView {
    let row_id = StoredValue::new(link.id.clone());
    let role_badge = link.role;
    let status = link.case_status;
    let primary = link.is_primary;
    let can_edit = link.can_edit;
    let note_text = link.note.clone();
    let has_note = !note_text.trim().is_empty();

    let role = RwSignal::new(link.role.slug().to_string());
    let note = RwSignal::new(link.note.clone());
    let is_primary = RwSignal::new(link.is_primary);
    let editing = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(String::new());

    let seed_role = StoredValue::new(link.role.slug().to_string());
    let seed_note = StoredValue::new(link.note.clone());
    let seed_primary = link.is_primary;
    let name = link.case_name.clone();

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

    let edit_label = StoredValue::new(format!("Edit {name} link"));
    let remove_label = StoredValue::new(format!("Remove {name} link"));

    view! {
        <div
            class="rounded-lg border border-slate-800 bg-slate-950 p-3"
            aria-busy=move || busy.get().to_string()
        >
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div class="min-w-0">
                    <div class="flex flex-wrap items-center gap-2">
                        <span class="text-sm font-semibold text-slate-100">
                            {link.case_name.clone()}
                        </span>
                        <span class=badge_pill(status.badge_classes())>{status.label()}</span>
                        <span class=badge_pill(role_badge.badge_classes())>{role_badge.label()}</span>
                        <Show when=move || primary>
                            <span class=badge_pill("bg-primary-500/15 text-primary-300 ring-1 ring-primary-500/30")>
                                "Primary"
                            </span>
                        </Show>
                        <Show when=move || !can_edit>
                            <span class=badge_pill("bg-slate-700/40 text-slate-300 ring-1 ring-slate-600")>
                                "Read only"
                            </span>
                        </Show>
                    </div>
                    <p class="mt-1 text-xs text-slate-500">{link.case_id.clone()}</p>
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
                        id_prefix=format!("contact-case-{}", row_id.get_value())
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
pub fn ContactCasesPanel(contact_id: String) -> impl IntoView {
    let id = StoredValue::new(contact_id.clone());
    let add_region_id = StoredValue::new(format!("contact-case-add-{contact_id}"));
    let search_id = StoredValue::new(format!("contact-case-search-{contact_id}"));
    let results_id = StoredValue::new(format!("contact-case-results-{contact_id}"));
    let add_prefix = StoredValue::new(format!("contact-case-new-{contact_id}"));

    let items = RwSignal::new(Vec::<ContactCaseLink>::new());
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
    let search_results = RwSignal::new(Vec::<EditableCaseSummary>::new());
    let selected_case_id = RwSignal::new(String::new());
    let selected_label = RwSignal::new(String::new());

    let role = RwSignal::new(CaseContactRole::default().slug().to_string());
    let note = RwSignal::new(String::new());
    let is_primary = RwSignal::new(false);

    Effect::new(move |_| {
        reload.track();
        loading.set(true);
        spawn_local(async move {
            match list_contact_cases(id.get_value()).await {
                Ok(list) => {
                    items.set(list);
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
            let response = search_editable_cases(query).await;
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
        selected_case_id.set(String::new());
        selected_label.set(String::new());
        role.set(CaseContactRole::default().slug().to_string());
        note.set(String::new());
        is_primary.set(false);
    };

    let submit = move |_| {
        if busy.get_untracked() {
            return;
        }
        let case_id = selected_case_id.get_untracked();
        if case_id.is_empty() {
            error.set("Choose a case to link.".into());
            return;
        }
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = add_case_contact(
                case_id,
                id.get_value(),
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
                    selected_case_id.set(String::new());
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
                    "Type a case name or ID to search the cases you can edit."
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
        let results = search_results.get();
        if results.is_empty() {
            return view! {
                <div
                    id=results_id.get_value()
                    role="listbox"
                    class="absolute z-10 mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-xs text-slate-500"
                >
                    "No matching editable cases."
                </div>
            }
            .into_any();
        }
        let rows = results
            .into_iter()
            .enumerate()
            .map(|(index, case)| {
                let option_id = format!("{}-{index}", results_id.get_value());
                let case_id = case.id.clone();
                let case_label = case.name.clone();
                let select = move |_| {
                    selected_case_id.set(case_id.clone());
                    selected_label.set(case_label.clone());
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
                            "block w-full bg-slate-800 px-3 py-1.5 text-left text-sm text-slate-100"
                        } else {
                            "block w-full px-3 py-1.5 text-left text-sm text-slate-200 hover:bg-slate-800"
                        }
                    >
                        <div class="truncate">{case.name}</div>
                        <div class="mt-0.5 flex flex-wrap items-center gap-2 text-xs text-slate-500">
                            <span>{case.id}</span>
                            <span class=badge_pill(case.status.badge_classes())>{case.status.label()}</span>
                        </div>
                    </button>
                }
            })
            .collect_view();
        view! {
            <div
                id=results_id.get_value()
                role="listbox"
                class="absolute z-10 mt-1 max-h-56 w-full overflow-y-auto rounded-xl border border-slate-700 bg-slate-900"
            >
                {rows}
            </div>
        }
        .into_any()
    };

    let rows = move || {
        let list = items.get();
        if list.is_empty() {
            let message = if loading.get() {
                "Loading cases…"
            } else {
                "This person is not linked to any cases yet."
            };
            return view! { <p class="text-sm text-slate-500">{message}</p> }.into_any();
        }
        list.into_iter()
            .map(|link| {
                view! {
                    <ContactCaseRow
                        link
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
                if debounce_generation.get_untracked() == generation {
                    debounced_search_query.set(value);
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
                    <h3 class="text-sm font-semibold text-slate-200">"Cases involving this person"</h3>
                    <p class="mt-1 text-sm text-slate-500">
                        "Read through admin case detail, and manage links only on cases you can edit."
                    </p>
                </div>
                <button
                    type="button"
                    on:click=toggle_adding
                    aria-expanded=move || adding.get().to_string()
                    aria-controls=add_region_id.get_value()
                    class="shrink-0 rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600"
                >
                    {move || if adding.get() { "Cancel" } else { "+ Add case" }}
                </button>
            </div>

            <Show when=move || adding.get()>
                <div id=add_region_id.get_value() class="mt-4 space-y-3 border-t border-slate-800 pt-4">
                    <div class="relative">
                        <label class="block" for=search_id.get_value()>
                            <span class=LABEL>"Case"</span>
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
                                placeholder="Search by case name or ID"
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
                                    selected_case_id.set(String::new());
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
                                                if let Some(case) = search_results.get_untracked().get(index).cloned() {
                                                    selected_case_id.set(case.id);
                                                    selected_label.set(case.name);
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
                        "Only cases where you currently hold EditCase appear in search results."
                    </p>

                    <button
                        type="button"
                        on:click=submit
                        prop:disabled=move || busy.get() || selected_case_id.get().is_empty()
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
