//! The property filter bar: pick a property, tick the values in use, get a chip.
//!
//! Shared by the Contacts directory, the Organizations directory, and the mail
//! recipient picker, because all three ask the same question of records that
//! happen to live in different tables.
//!
//! Counts come from the server against the filters already applied, so a value
//! that would return nothing is simply not offered. The bar owns no URL state:
//! the pages decide whether their filters are worth putting in a link.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::server_fns::err_text;
use crate::server_fns::property_filters::{
    list_property_facets, PropertyFacet, PropertyFacetScope, PropertyFilter, PropertySubject,
};

const INPUT: &str = "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40";

/// Which pane of the popover is open.
#[derive(Clone, PartialEq)]
enum Pane {
    Closed,
    /// Choosing a property name.
    Keys,
    /// Choosing values for the named property.
    Values(String),
}

#[component]
pub fn PropertyFilterBar(
    subject: PropertySubject,
    /// The applied chips. The parent watches this to re-run its own search.
    filters: RwSignal<Vec<PropertyFilter>>,
    /// Everything else the parent has filtered by, so counts match the screen.
    scope: Signal<PropertyFacetScope>,
) -> impl IntoView {
    let facets = RwSignal::new(Vec::<PropertyFacet>::new());
    let pane = RwSignal::new(Pane::Closed);
    let key_search = RwSignal::new(String::new());
    let value_search = RwSignal::new(String::new());
    // The values ticked in the open pane, committed only on Apply.
    let draft = RwSignal::new(Vec::<String>::new());
    let loading = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let generation = RwSignal::new(0u64);

    // Refetch whenever the surrounding filters change, so the counts on offer
    // always describe the list the reader is looking at.
    Effect::new(move |_| {
        let mut current = scope.get();
        current.property_filters = filters.get();
        generation.update(|value| *value += 1);
        let this = generation.get_untracked();
        loading.set(true);
        spawn_local(async move {
            match list_property_facets(subject, current).await {
                Ok(found) if generation.get_untracked() == this => {
                    facets.set(found);
                    error.set(String::new());
                }
                Err(e) if generation.get_untracked() == this => error.set(err_text(e)),
                _ => return,
            }
            loading.set(false);
        });
    });

    let facet_for = move |key: &str| {
        facets
            .get()
            .into_iter()
            .find(|facet| facet.key == key)
            .unwrap_or_default()
    };

    let open_values = move |key: String| {
        draft.set(
            filters
                .get_untracked()
                .into_iter()
                .find(|filter| filter.key == key)
                .map(|filter| filter.values)
                .unwrap_or_default(),
        );
        value_search.set(String::new());
        pane.set(Pane::Values(key));
    };

    let apply = move |key: String| {
        let values = draft.get_untracked();
        filters.update(|list| {
            list.retain(|filter| filter.key != key);
            if !values.is_empty() {
                list.push(PropertyFilter { key, values });
            }
        });
        pane.set(Pane::Closed);
    };

    // Derived outside the view: a bare `>` inside the template reads as a tag close.
    let many_filters = Signal::derive(move || filters.get().len() > 1);

    let chips = move || {
        filters
            .get()
            .into_iter()
            .map(|filter| {
                let facet = facet_for(&filter.key);
                let label = if facet.display_key.is_empty() {
                    filter.key.clone()
                } else {
                    facet.display_key.clone()
                };
                // Show what was picked, not just how many, so the chip reads as
                // a sentence: "Location: Boston, Cambridge".
                let picked = filter
                    .values
                    .iter()
                    .map(|value| {
                        facet
                            .values
                            .iter()
                            .find(|candidate| &candidate.value == value)
                            .map(|candidate| candidate.label())
                            .unwrap_or_else(|| {
                                if value.is_empty() {
                                    "\u{2014} not filled in \u{2014}".to_string()
                                } else {
                                    value.clone()
                                }
                            })
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let edit_key = filter.key.clone();
                let remove_key = filter.key.clone();
                view! {
                    <span class="inline-flex items-center gap-1 rounded-full bg-primary-500/15 py-1 pl-3 pr-1 text-xs text-primary-200 ring-1 ring-primary-500/30">
                        <button
                            type="button"
                            on:click=move |_| open_values(edit_key.clone())
                            class="hover:text-primary-100"
                        >
                            <span class="font-semibold">{label}</span>
                            ": "
                            {picked}
                        </button>
                        <button
                            type="button"
                            aria-label="Remove filter"
                            on:click=move |_| {
                                let key = remove_key.clone();
                                filters.update(|list| list.retain(|filter| filter.key != key));
                                pane.set(Pane::Closed);
                            }
                            class="rounded-full px-1.5 text-primary-300 hover:bg-primary-500/20 hover:text-primary-100"
                        >
                            "\u{00d7}"
                        </button>
                    </span>
                }
            })
            .collect_view()
    };

    let key_pane = move || {
        let needle = key_search.get().trim().to_lowercase();
        let active: Vec<String> = filters.get().into_iter().map(|filter| filter.key).collect();
        let matches: Vec<PropertyFacet> = facets
            .get()
            .into_iter()
            .filter(|facet| {
                !active.contains(&facet.key) && (needle.is_empty() || facet.key.contains(&needle))
            })
            .collect();
        if matches.is_empty() {
            let message = if loading.get() {
                "Loading properties\u{2026}"
            } else if facets.get().is_empty() {
                "No properties are recorded on these records yet."
            } else {
                "Every matching property is already filtered."
            };
            return view! { <p class="px-3 py-2 text-xs text-slate-500">{message}</p> }.into_any();
        }
        matches
            .into_iter()
            .map(|facet| {
                let key = facet.key.clone();
                let count = facet.record_count;
                view! {
                    <button
                        type="button"
                        on:click=move |_| open_values(key.clone())
                        class="flex w-full items-baseline justify-between gap-3 rounded-md px-3 py-2 text-left text-xs text-slate-300 hover:bg-slate-800"
                    >
                        <span class="truncate">{facet.display_key}</span>
                        <span class="shrink-0 text-[0.68rem] text-slate-500">{count}</span>
                    </button>
                }
            })
            .collect_view()
            .into_any()
    };

    let value_pane = move |key: String| {
        let facet = facet_for(&key);
        let heading = facet.display_key.clone();
        let truncated = facet.truncated;
        let distinct = facet.distinct_values;
        let apply_key = key.clone();
        let needle = value_search.get().trim().to_lowercase();
        let rows = facet
            .values
            .into_iter()
            .filter(|candidate| needle.is_empty() || candidate.label().to_lowercase().contains(&needle))
            .map(|candidate| {
                let value = candidate.value.clone();
                let checked_value = value.clone();
                let blank = value.is_empty();
                let label = candidate.label();
                let count = candidate.count;
                view! {
                    <label class="flex cursor-pointer items-center justify-between gap-3 rounded-md px-3 py-1.5 text-xs text-slate-300 hover:bg-slate-800">
                        <span class="flex min-w-0 items-center gap-2">
                            <input
                                type="checkbox"
                                class="shrink-0 accent-primary-500"
                                prop:checked=move || draft.get().contains(&checked_value)
                                on:change=move |event| {
                                    let checked = event_target_checked(&event);
                                    let value = value.clone();
                                    draft.update(|list| {
                                        if checked {
                                            if !list.contains(&value) {
                                                list.push(value);
                                            }
                                        } else {
                                            list.retain(|selected| selected != &value);
                                        }
                                    });
                                }
                            />
                            <span class=if blank { "truncate italic text-slate-500" } else { "truncate" }>
                                {label}
                            </span>
                        </span>
                        <span class="shrink-0 text-[0.68rem] text-slate-500">{count}</span>
                    </label>
                }
            })
            .collect_view();

        view! {
            <div>
                <div class="flex items-center justify-between gap-2 border-b border-slate-800 px-3 py-2">
                    <button
                        type="button"
                        on:click=move |_| pane.set(Pane::Keys)
                        class="text-xs text-slate-400 hover:text-slate-200"
                    >
                        "\u{2190} All properties"
                    </button>
                    <span class="truncate text-xs font-semibold text-slate-200">{heading}</span>
                </div>
                <div class="px-3 pt-2">
                    <input
                        class=INPUT
                        placeholder="Find a value\u{2026}"
                        prop:value=move || value_search.get()
                        on:input=move |event| value_search.set(event_target_value(&event))
                    />
                </div>
                <div class="max-h-56 overflow-y-auto py-1">{rows}</div>
                <Show when=move || truncated>
                    <p class="px-3 pb-1 text-[0.68rem] text-slate-500">
                        "Showing the most common values of " {distinct} "."
                    </p>
                </Show>
                <div class="flex items-center justify-between gap-2 border-t border-slate-800 px-3 py-2">
                    <button
                        type="button"
                        on:click=move |_| draft.set(Vec::new())
                        class="text-xs text-slate-400 hover:text-slate-200"
                    >
                        "Clear"
                    </button>
                    <button
                        type="button"
                        on:click=move |_| apply(apply_key.clone())
                        class="rounded-full bg-primary-500 px-3 py-1.5 text-xs font-semibold text-white hover:bg-primary-600"
                    >
                        {move || {
                            let count = draft.get().len();
                            if count == 0 {
                                "Apply".to_string()
                            } else {
                                format!("Apply ({count})")
                            }
                        }}
                    </button>
                </div>
            </div>
        }
        .into_any()
    };

    view! {
        <div class="space-y-2">
            <div class="flex flex-wrap items-center gap-2">
                <div class="relative">
                    <button
                        type="button"
                        on:click=move |_| {
                            if pane.get_untracked() == Pane::Closed {
                                key_search.set(String::new());
                                pane.set(Pane::Keys);
                            } else {
                                pane.set(Pane::Closed);
                            }
                        }
                        class="rounded-full border border-slate-700 px-3 py-1.5 text-xs font-medium text-slate-300 hover:bg-slate-800"
                    >
                        "+ Add property filter"
                    </button>

                    <Show when=move || pane.get() != Pane::Closed>
                        <div class="absolute left-0 z-20 mt-2 w-72 rounded-xl border border-slate-700 bg-slate-900 shadow-xl">
                            {move || match pane.get() {
                                Pane::Values(key) => value_pane(key),
                                _ => view! {
                                    <div>
                                        <div class="px-3 pt-3">
                                            <input
                                                class=INPUT
                                                placeholder="Find a property\u{2026}"
                                                prop:value=move || key_search.get()
                                                on:input=move |event| key_search.set(event_target_value(&event))
                                            />
                                        </div>
                                        <div class="max-h-64 overflow-y-auto py-2">{key_pane}</div>
                                    </div>
                                }
                                .into_any(),
                            }}
                        </div>
                    </Show>
                </div>

                {chips}

                <Show when=move || many_filters.get()>
                    <button
                        type="button"
                        on:click=move |_| {
                            filters.set(Vec::new());
                            pane.set(Pane::Closed);
                        }
                        class="text-xs text-slate-400 hover:text-slate-200"
                    >
                        "Clear all"
                    </button>
                </Show>
            </div>

            <Show when=move || !error.get().is_empty()>
                <p class="text-xs text-rose-300" role="alert">{move || error.get()}</p>
            </Show>
        </div>
    }
}

/// The filtered property values on one result card, so a row can say why it matched.
#[component]
pub fn MatchedProperties(properties: Vec<(String, String)>) -> impl IntoView {
    if properties.is_empty() {
        return ().into_any();
    }
    properties
        .into_iter()
        .map(|(key, value)| {
            let blank = value.trim().is_empty();
            view! {
                <span class="inline-flex items-center gap-1 rounded-full bg-slate-800 px-2 py-0.5 text-[0.68rem] text-slate-300">
                    <span class="text-slate-500">{key}</span>
                    <span class=if blank { "italic text-slate-500" } else { "" }>
                        {if blank { "not filled in".to_string() } else { value }}
                    </span>
                </span>
            }
        })
        .collect_view()
        .into_any()
}
