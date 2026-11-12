//! The custom-property editor on an organization, grouped into sections.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::helpers::sections;
use crate::server_fns::err_text;
use crate::server_fns::organization_properties::{
    add_missing_organization_property_defaults, list_organization_properties,
    set_organization_properties, OrganizationProperty,
};
use crate::state::AppState;

const PANEL: &str = "rounded-card border border-slate-800 bg-slate-900 shadow-card p-5";
const INPUT: &str =
    "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-600 focus:border-primary-500 focus:outline-none";

#[derive(Clone)]
struct Row {
    id: usize,
    key: String,
    value: String,
    section: String,
}

#[component]
pub fn OrganizationPropertiesPanel(organization_id: String) -> impl IntoView {
    let state = expect_context::<AppState>();
    let can_edit = state.has_information_management_access();
    let id = StoredValue::new(organization_id);
    let saved = RwSignal::new(Vec::<OrganizationProperty>::new());
    let rows = RwSignal::new(Vec::<Row>::new());
    let next_row_id = RwSignal::new(0usize);
    let editing = RwSignal::new(false);
    let loading = RwSignal::new(true);
    let saving = RwSignal::new(false);
    let adding_defaults = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let reload = RwSignal::new(0u32);

    Effect::new(move |_| {
        reload.track();
        loading.set(true);
        spawn_local(async move {
            match list_organization_properties(id.get_value()).await {
                Ok(list) => {
                    saved.set(list);
                    error.set(String::new());
                }
                Err(e) => error.set(err_text(e)),
            }
            loading.set(false);
        });
    });

    let new_row = move |key: String, value: String, section: String| {
        let row_id = next_row_id.get_untracked();
        next_row_id.set(row_id + 1);
        Row {
            id: row_id,
            key,
            value,
            section,
        }
    };

    let begin_edit = move |_| {
        let current: Vec<Row> = saved
            .get_untracked()
            .into_iter()
            .map(|p| new_row(p.key, p.value, p.section))
            .collect();
        rows.set(current);
        editing.set(true);
        error.set(String::new());
    };

    let add_row = move |_| {
        rows.update(|list| list.push(new_row(String::new(), String::new(), String::new())))
    };

    let save = move |_| {
        if saving.get_untracked() || adding_defaults.get_untracked() {
            return;
        }
        let properties: Vec<OrganizationProperty> = rows
            .get_untracked()
            .into_iter()
            .map(|row| OrganizationProperty {
                key: row.key,
                value: row.value,
                section: row.section,
            })
            .collect();
        saving.set(true);
        error.set(String::new());
        spawn_local(async move {
            match set_organization_properties(id.get_value(), properties).await {
                Ok(()) => {
                    editing.set(false);
                    reload.update(|r| *r += 1);
                }
                Err(e) => error.set(err_text(e)),
            }
            saving.set(false);
        });
    };

    let add_defaults = move |_| {
        if saving.get_untracked() || adding_defaults.get_untracked() {
            return;
        }
        adding_defaults.set(true);
        error.set(String::new());
        spawn_local(async move {
            match add_missing_organization_property_defaults(id.get_value()).await {
                Ok(()) => reload.update(|r| *r += 1),
                Err(e) => error.set(err_text(e)),
            }
            adding_defaults.set(false);
        });
    };

    let grouped = move || {
        let list = saved.get();
        let mut order: Vec<String> = Vec::new();
        for property in &list {
            if !order.contains(&property.section) {
                order.push(property.section.clone());
            }
        }
        order
            .into_iter()
            .map(|section| {
                let in_section: Vec<OrganizationProperty> = list
                    .iter()
                    .filter(|p| p.section == section)
                    .cloned()
                    .collect();
                (sections::label(&section).to_string(), in_section)
            })
            .collect::<Vec<_>>()
    };

    let read_view = move || {
        let groups = grouped();
        if groups.is_empty() {
            let message = if loading.get() {
                "Loading properties…"
            } else {
                "No properties recorded yet."
            };
            return view! { <p class="text-sm text-slate-500">{message}</p> }.into_any();
        }
        groups
            .into_iter()
            .map(|(heading, properties)| {
                view! {
                    <section class="border-t border-slate-800 pt-3 first:border-t-0 first:pt-0">
                        <h4 class="text-xs font-semibold uppercase tracking-wide text-slate-500">{heading}</h4>
                        <dl class="mt-2 grid gap-x-6 sm:grid-cols-2">
                            {properties
                                .into_iter()
                                .map(|p| {
                                    let blank = p.value.trim().is_empty();
                                    view! {
                                        <div class="border-b border-slate-800/60 py-2">
                                            <dt class="wrap-anywhere text-xs font-medium text-slate-500">{p.key}</dt>
                                            <dd class=if blank {
                                                "mt-0.5 wrap-anywhere text-sm italic text-slate-600"
                                            } else {
                                                "mt-0.5 wrap-anywhere whitespace-pre-wrap text-sm text-slate-200"
                                            }>
                                                {if blank { "Not filled in".to_string() } else { p.value }}
                                            </dd>
                                        </div>
                                    }
                                })
                                .collect_view()}
                        </dl>
                    </section>
                }
            })
            .collect_view()
            .into_any()
    };

    let edit_view = move || {
        view! {
            <div class="space-y-2">
                <For each=move || rows.get() key=|row| row.id let:row>
                    {
                        let row_id = row.id;
                        let field_value = move |field: fn(&Row) -> &String| {
                            rows.with(|list| {
                                list.iter()
                                    .find(|candidate| candidate.id == row_id)
                                    .map(field)
                                    .cloned()
                                    .unwrap_or_default()
                            })
                        };
                        view! {
                            <div class="grid gap-2 sm:grid-cols-[1fr_1fr_1fr_auto]">
                                <input
                                    class=INPUT
                                    placeholder="Property"
                                    prop:value=move || field_value(|row| &row.key)
                                    on:input=move |e| rows.update(|list| {
                                        if let Some(row) = list.iter_mut().find(|row| row.id == row_id) {
                                            row.key = event_target_value(&e);
                                        }
                                    })
                                />
                                <input
                                    class=INPUT
                                    placeholder="Value"
                                    prop:value=move || field_value(|row| &row.value)
                                    on:input=move |e| rows.update(|list| {
                                        if let Some(row) = list.iter_mut().find(|row| row.id == row_id) {
                                            row.value = event_target_value(&e);
                                        }
                                    })
                                />
                                <input
                                    class=INPUT
                                    placeholder="Section"
                                    prop:value=move || field_value(|row| &row.section)
                                    on:input=move |e| rows.update(|list| {
                                        if let Some(row) = list.iter_mut().find(|row| row.id == row_id) {
                                            row.section = event_target_value(&e);
                                        }
                                    })
                                />
                                <button
                                    type="button"
                                    on:click=move |_| rows.update(|list| list.retain(|row| row.id != row_id))
                                    class="rounded-full border border-slate-700 px-3 py-2 text-xs font-medium text-slate-400 hover:bg-slate-800"
                                >
                                    "Remove"
                                </button>
                            </div>
                        }
                    }
                </For>
                <button
                    type="button"
                    on:click=add_row
                    class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-300 hover:bg-slate-800"
                >
                    "+ Add property"
                </button>
            </div>
        }
    };

    view! {
        <div class=PANEL>
            <div class="flex items-start justify-between gap-3">
                <div>
                    <h3 class="text-sm font-semibold text-slate-200">"Properties"</h3>
                    <p class="mt-1 text-xs text-slate-500">
                        "Custom fields about this organization, grouped into sections."
                    </p>
                </div>
                <div class="flex shrink-0 gap-2">
                    <Show
                        when=move || editing.get()
                        fallback=move || {
                            if !can_edit {
                                return ().into_any();
                            }
                            view! {
                                <>
                                    <button
                                        type="button"
                                        on:click=add_defaults
                                        prop:disabled=move || adding_defaults.get() || loading.get()
                                        class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-300 hover:bg-slate-800 disabled:opacity-50"
                                    >
                                        {move || if adding_defaults.get() {
                                            "Adding defaults…"
                                        } else {
                                            "Add missing defaults"
                                        }}
                                    </button>
                                    <button
                                        type="button"
                                        on:click=begin_edit
                                        prop:disabled=move || loading.get() || adding_defaults.get()
                                        class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800 disabled:opacity-50"
                                    >
                                        "Edit"
                                    </button>
                                </>
                            }
                            .into_any()
                        }
                    >
                        <button
                            type="button"
                            on:click=save
                            prop:disabled=move || saving.get()
                            class="rounded-full bg-primary-500 px-3 py-1.5 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                        >
                            {move || if saving.get() { "Saving…" } else { "Save" }}
                        </button>
                        <button
                            type="button"
                            on:click=move |_| editing.set(false)
                            class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-300 hover:bg-slate-800"
                        >
                            "Cancel"
                        </button>
                    </Show>
                </div>
            </div>

            <Show when=move || !error.get().is_empty()>
                <p class="mt-3 text-sm text-rose-300" role="alert">{move || error.get()}</p>
            </Show>

            <div class="mt-4">
                <Show when=move || editing.get() fallback=read_view>
                    {edit_view()}
                </Show>
            </div>
        </div>
    }
}
