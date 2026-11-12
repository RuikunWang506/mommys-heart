//! An editable property list for a record that does not exist yet.
//!
//! The properties panel on a saved contact loads and writes its own rows. When
//! a contact is being created there is nothing to load from, so the same fields
//! have to be collected on the form and sent with it — otherwise the first a
//! user sees of them is after saving, on a second visit to the record.
//!
//! Rows a rule contributes are shown as named fields rather than free-form
//! key/value pairs: their name is not the user's to change, and a required one
//! cannot be removed.

use leptos::prelude::*;

use crate::helpers::sections;
use crate::server_fns::contact_properties::ContactProperty;
use crate::server_fns::contact_rules::RuleField;

const INPUT: &str = "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-600 focus:border-primary-500 focus:outline-none";

/// One row being edited. `id` distinguishes rows within the editor so adding or
/// removing one never reorders the others.
#[derive(Clone, Debug, PartialEq)]
pub struct PropertyRow {
    pub id: usize,
    pub key: String,
    pub value: String,
    pub section: String,
    /// Contributed by a rule: the name is fixed.
    pub managed: bool,
    /// A value must be supplied before the contact can be created.
    pub required: bool,
}

impl PropertyRow {
    pub fn to_property(&self) -> ContactProperty {
        ContactProperty {
            key: self.key.clone(),
            value: self.value.clone(),
            section: self.section.clone(),
        }
    }
}

/// Seed the editor with the code-owned defaults, then whatever the matching
/// rules add on top.
pub fn seed_rows(fields: &[RuleField], next_id: &mut usize) -> Vec<PropertyRow> {
    let mut rows = Vec::new();
    for property in crate::server_fns::contact_properties::default_properties() {
        rows.push(PropertyRow {
            id: {
                *next_id += 1;
                *next_id
            },
            key: property.key,
            value: property.value,
            section: property.section,
            managed: false,
            required: false,
        });
    }
    merge_rule_fields(&mut rows, fields, next_id);
    rows
}

/// Bring the row list in line with the rules that now apply.
///
/// Fields that arrived are appended; fields that no longer apply are dropped
/// only when still blank, so a value someone typed is never silently lost by
/// unticking a category.
pub fn merge_rule_fields(rows: &mut Vec<PropertyRow>, fields: &[RuleField], next_id: &mut usize) {
    rows.retain(|row| {
        !row.managed
            || !row.value.trim().is_empty()
            || fields.iter().any(|field| field.matches(&row.section, &row.key))
    });

    for field in fields {
        if let Some(existing) = rows
            .iter_mut()
            .find(|row| field.matches(&row.section, &row.key))
        {
            // A rule may have changed whether the field is demanded.
            existing.managed = true;
            existing.required = field.required;
            continue;
        }
        *next_id += 1;
        rows.push(PropertyRow {
            id: *next_id,
            key: field.key.clone(),
            value: field.default_value.clone(),
            section: field.section.clone(),
            managed: true,
            required: field.required,
        });
    }
}

/// Required rows still waiting for a value, named for a message.
pub fn unfilled_required(rows: &[PropertyRow]) -> Vec<String> {
    rows.iter()
        .filter(|row| row.required && row.value.trim().is_empty())
        .map(|row| {
            if row.section.trim().is_empty() {
                row.key.clone()
            } else {
                format!("{} / {}", row.section.trim(), row.key.trim())
            }
        })
        .collect()
}

#[component]
pub fn PropertyRowsEditor(rows: RwSignal<Vec<PropertyRow>>, next_id: RwSignal<usize>) -> impl IntoView {
    let add_row = move |_| {
        next_id.update(|id| *id += 1);
        let id = next_id.get_untracked();
        rows.update(|list| {
            list.push(PropertyRow {
                id,
                key: String::new(),
                value: String::new(),
                section: String::new(),
                managed: false,
                required: false,
            })
        });
    };

    // Grouped by section in first-seen order, matching how the saved record
    // displays them, so the form previews the record it is about to create.
    let grouped = move || {
        let list = rows.get();
        let mut order: Vec<String> = Vec::new();
        for row in &list {
            if !order.contains(&row.section) {
                order.push(row.section.clone());
            }
        }
        order
            .into_iter()
            .map(|section| {
                let in_section: Vec<PropertyRow> = list
                    .iter()
                    .filter(|row| row.section == section)
                    .cloned()
                    .collect();
                (sections::label(&section).to_string(), in_section)
            })
            .collect::<Vec<_>>()
    };

    let field_value = move |row_id: usize, pick: fn(&PropertyRow) -> &String| {
        rows.with(|list| {
            list.iter()
                .find(|row| row.id == row_id)
                .map(pick)
                .cloned()
                .unwrap_or_default()
        })
    };

    view! {
        <div class="space-y-4">
            {move || grouped()
                .into_iter()
                .map(|(heading, group)| view! {
                    <section>
                        <h4 class="text-xs font-semibold uppercase tracking-wide text-slate-500">
                            {heading}
                        </h4>
                        <div class="mt-2 space-y-2">
                            {group
                                .into_iter()
                                .map(|row| {
                                    let row_id = row.id;
                                    let managed = row.managed;
                                    let required = row.required;
                                    view! {
                                        <div class="grid gap-2 sm:grid-cols-[1fr_1fr_auto]">
                                            <Show
                                                when=move || managed
                                                fallback=move || view! {
                                                    <input
                                                        class=INPUT
                                                        placeholder="Property"
                                                        prop:value=move || field_value(row_id, |row| &row.key)
                                                        on:input=move |e| rows.update(|list| {
                                                            if let Some(row) = list.iter_mut().find(|row| row.id == row_id) {
                                                                row.key = event_target_value(&e);
                                                            }
                                                        })
                                                    />
                                                }
                                            >
                                                <span class="flex items-center gap-1 px-1 py-2 text-sm text-slate-300">
                                                    {field_value(row_id, |row| &row.key)}
                                                    {required.then(|| view! {
                                                        <span class="text-rose-300" title="Required">"*"</span>
                                                    })}
                                                </span>
                                            </Show>
                                            <input
                                                class=move || {
                                                    let blank = field_value(row_id, |row| &row.value).trim().is_empty();
                                                    if required && blank {
                                                        format!("{INPUT} border-rose-500/60")
                                                    } else {
                                                        INPUT.to_string()
                                                    }
                                                }
                                                placeholder=if required { "Required" } else { "Value" }
                                                prop:value=move || field_value(row_id, |row| &row.value)
                                                on:input=move |e| rows.update(|list| {
                                                    if let Some(row) = list.iter_mut().find(|row| row.id == row_id) {
                                                        row.value = event_target_value(&e);
                                                    }
                                                })
                                            />
                                            <Show
                                                when=move || !required
                                                fallback=move || view! {
                                                    <span class="px-3 py-2 text-xs text-slate-500">"Required"</span>
                                                }
                                            >
                                                <button
                                                    type="button"
                                                    on:click=move |_| rows.update(|list| list.retain(|row| row.id != row_id))
                                                    class="rounded-full border border-slate-700 px-3 py-2 text-xs font-medium text-slate-400 hover:bg-slate-800"
                                                >
                                                    "Remove"
                                                </button>
                                            </Show>
                                        </div>
                                    }
                                })
                                .collect_view()}
                        </div>
                    </section>
                })
                .collect_view()}

            <button
                type="button"
                on:click=add_row
                class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-300 hover:bg-slate-800"
            >
                "+ Add property"
            </button>
        </div>
    }
}
