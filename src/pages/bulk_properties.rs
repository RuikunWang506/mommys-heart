//! The bulk property workspace: name one property, choose many records, save once.
//!
//! Laid out as the same numbered steps as the contact-mail page, and sharing its
//! selection semantics, so "filter, tick, or select everything matching" behaves
//! the same way in both tools.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::use_query_map;

use crate::components::guard::require_information_management_access;
use crate::components::layout::Layout;
use crate::components::property_filters::PropertyFilterBar;
use crate::server_fns::bulk_properties::{
    apply_bulk_property_edit, list_bulk_property_candidates, list_property_key_options,
    preview_bulk_property_edit, BulkPropertyCandidate, BulkPropertyEdit, BulkPropertyFilters,
    BulkPropertyOutcome, BulkPropertyPreview, BulkPropertySelection, PropertyKeyOption,
    PropertyRef, CANDIDATE_PAGE_SIZE,
};
use crate::server_fns::contacts::ContactType;
use crate::server_fns::err_text;
use crate::server_fns::organizations::{list_organizations, OrganizationFilters, OrganizationKind};
use crate::server_fns::property_filters::{PropertyFacetScope, PropertyFilter, PropertySubject};
use crate::state::AppState;

const INPUT: &str = "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40 disabled:cursor-not-allowed disabled:opacity-50";
const LABEL: &str = "mb-1 block text-xs font-medium text-slate-400";
const PANEL: &str = "rounded-card border border-slate-800 bg-slate-900 shadow-card p-5";

#[component]
pub fn BulkPropertiesPage() -> impl IntoView {
    let state = expect_context::<AppState>();
    require_information_management_access(state, move || {
        view! {
            <Layout title="Bulk edit properties".to_string()>
                <BulkPropertiesWorkspace />
            </Layout>
        }
        .into_any()
    })
}

/// Browser `confirm`, or a refusal during SSR where no dialog exists.
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

fn plural(count: i64, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

#[component]
fn BulkPropertiesWorkspace() -> impl IntoView {
    let query_map = use_query_map();
    let initial_subject = query_map
        .with_untracked(|map| map.get("subject").map(|value| value.to_string()))
        .and_then(|value| PropertySubject::from_slug(&value))
        .unwrap_or_default();

    let subject = RwSignal::new(initial_subject);

    // Step 1: the property being set.
    let section = RwSignal::new(String::new());
    let key = RwSignal::new(String::new());
    let value = RwSignal::new(String::new());
    let key_options = RwSignal::new(Vec::<PropertyKeyOption>::new());

    // Step 2: filters and selection.
    let keyword = RwSignal::new(String::new());
    let contact_type = RwSignal::new(String::new());
    let organization_id = RwSignal::new(String::new());
    let organization_kind = RwSignal::new(String::new());
    let include_archived = RwSignal::new(false);
    // The shared chips, owned by the filter bar below.
    let property_filters = RwSignal::new(Vec::<PropertyFilter>::new());
    let only_missing_target = RwSignal::new(false);
    let applied_filters = RwSignal::new(BulkPropertyFilters::default());
    // The property the applied list was keyed to, so the "current value" column
    // does not silently re-key while the user is still typing in step 1.
    let applied_target = RwSignal::new(PropertyRef::default());

    let selected_ids = RwSignal::new(Vec::<String>::new());
    let excluded_ids = RwSignal::new(Vec::<String>::new());
    let all_matching = RwSignal::new(false);

    let candidates = RwSignal::new(Vec::<BulkPropertyCandidate>::new());
    let total = RwSignal::new(0i64);
    let offset = RwSignal::new(0i64);
    let organizations = RwSignal::new(Vec::<(String, String)>::new());

    let loading = RwSignal::new(true);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let preview = RwSignal::new(None::<BulkPropertyPreview>);
    let outcome = RwSignal::new(None::<BulkPropertyOutcome>);
    let search_generation = RwSignal::new(0u64);
    let reload = RwSignal::new(0u32);

    let selected_count = Signal::derive(move || {
        if all_matching.get() {
            (total.get() - excluded_ids.get().len() as i64).max(0)
        } else {
            selected_ids.get().len() as i64
        }
    });
    // Kept out of the view: the `>=` comparison would be parsed as a tag close
    // inside the `view!` macro.
    let on_last_page =
        Signal::derive(move || offset.get() + CANDIDATE_PAGE_SIZE >= total.get());
    let has_applied_target = Signal::derive(move || applied_target.get().is_named());

    let current_property = move || PropertyRef {
        section: section.get(),
        key: key.get(),
    };

    // Facet counts are measured against the filters actually applied, so the
    // values on offer describe the list on screen rather than the whole table.
    let facet_scope = Signal::derive(move || {
        let applied = applied_filters.get();
        PropertyFacetScope {
            keyword: applied.keyword,
            include_archived: applied.include_archived,
            contact_type: applied
                .contact_type
                .map(|kind| kind.slug().to_string())
                .unwrap_or_default(),
            organization_id: applied.organization_id,
            organization_kind: applied
                .organization_kind
                .map(|kind| kind.slug().to_string())
                .unwrap_or_default(),
            ..Default::default()
        }
    });

    // Organizations only ever fill the people-side organization picker.
    Effect::new(move |_| {
        spawn_local(async move {
            if let Ok(page) = list_organizations(
                OrganizationFilters {
                    include_archived: true,
                    ..Default::default()
                },
                0,
                200,
            )
            .await
            {
                organizations.set(page.items.into_iter().map(|o| (o.id, o.name)).collect());
            }
        });
    });

    Effect::new(move |_| {
        let chosen = subject.get();
        spawn_local(async move {
            match list_property_key_options(chosen).await {
                Ok(options) => key_options.set(options),
                Err(server_error) => error.set(err_text(server_error)),
            }
        });
    });

    Effect::new(move |_| {
        let chosen = subject.get();
        let filters = applied_filters.get();
        let target = applied_target.get();
        let page_offset = offset.get();
        reload.track();
        search_generation.update(|generation| *generation += 1);
        let generation = search_generation.get_untracked();
        loading.set(true);
        spawn_local(async move {
            let result =
                list_bulk_property_candidates(chosen, filters, target, page_offset, CANDIDATE_PAGE_SIZE)
                    .await;
            if search_generation.get_untracked() != generation {
                return;
            }
            match result {
                Ok(page) => {
                    candidates.set(page.items);
                    total.set(page.total);
                    error.set(String::new());
                }
                Err(server_error) => error.set(err_text(server_error)),
            }
            loading.set(false);
        });
    });

    let clear_selection = move || {
        all_matching.set(false);
        selected_ids.set(Vec::new());
        excluded_ids.set(Vec::new());
        preview.set(None);
    };

    let switch_subject = move |next: PropertySubject| {
        if subject.get_untracked() == next {
            return;
        }
        subject.set(next);
        clear_selection();
        offset.set(0);
        contact_type.set(String::new());
        organization_id.set(String::new());
        organization_kind.set(String::new());
        property_filters.set(Vec::new());
        applied_filters.set(BulkPropertyFilters::default());
        applied_target.set(PropertyRef::default());
        outcome.set(None);
        notice.set(String::new());
    };

    // Takes the property explicitly so the missing-target clause and the
    // displayed current-value column can never be keyed to different properties.
    let build_filters = move |property: &PropertyRef| BulkPropertyFilters {
        keyword: keyword.get_untracked(),
        contact_type: ContactType::from_slug(&contact_type.get_untracked()),
        organization_id: organization_id.get_untracked(),
        organization_kind: OrganizationKind::from_slug(&organization_kind.get_untracked()),
        include_archived: include_archived.get_untracked(),
        property_filters: property_filters.get_untracked(),
        // Only meaningful once a property has been named.
        only_missing_target: only_missing_target.get_untracked() && property.is_named(),
    };

    let apply_filters = move |_| {
        let property = current_property();
        offset.set(0);
        clear_selection();
        outcome.set(None);
        applied_filters.set(build_filters(&property));
        applied_target.set(property);
    };

    let build_selection = move || BulkPropertySelection {
        all_matching: all_matching.get_untracked(),
        filters: applied_filters.get_untracked(),
        ids: selected_ids.get_untracked(),
        excluded_ids: excluded_ids.get_untracked(),
    };

    let run_preview = move |_| {
        if busy.get_untracked() {
            return;
        }
        let property = current_property();
        if !property.is_named() {
            error.set("Name the property to set.".to_string());
            return;
        }
        if selected_count.get_untracked() == 0 {
            error.set("Choose at least one record.".to_string());
            return;
        }
        let chosen = subject.get_untracked();
        let selection = build_selection();
        let edit = BulkPropertyEdit {
            property,
            value: value.get_untracked(),
        };
        busy.set(true);
        error.set(String::new());
        notice.set(String::new());
        outcome.set(None);
        spawn_local(async move {
            match preview_bulk_property_edit(chosen, selection, edit).await {
                Ok(result) => preview.set(Some(result)),
                Err(server_error) => {
                    preview.set(None);
                    error.set(err_text(server_error));
                }
            }
            busy.set(false);
        });
    };

    let apply_edit = move |_| {
        if busy.get_untracked() {
            return;
        }
        let property = current_property();
        if !property.is_named() {
            error.set("Name the property to set.".to_string());
            return;
        }
        let count = selected_count.get_untracked();
        if count == 0 {
            error.set("Choose at least one record.".to_string());
            return;
        }
        let chosen = subject.get_untracked();
        let selection = build_selection();
        let new_value = value.get_untracked();
        let edit = BulkPropertyEdit {
            property: property.clone(),
            value: new_value.clone(),
        };
        let shown_value = if new_value.trim().is_empty() {
            "an empty value".to_string()
        } else {
            format!("\"{}\"", new_value.trim())
        };
        if !confirm(&format!(
            "Set {} to {shown_value} on {}?",
            property.label(),
            plural(count, "record", "records"),
        )) {
            return;
        }
        busy.set(true);
        error.set(String::new());
        notice.set(String::new());
        spawn_local(async move {
            match apply_bulk_property_edit(chosen, selection, edit).await {
                Ok(result) => {
                    notice.set(format!(
                        "Saved. {} changed, {} already had this value.",
                        plural(result.changed(), "record", "records"),
                        result.unchanged,
                    ));
                    outcome.set(Some(result));
                    preview.set(None);
                    // Re-run the search so the current-value column reflects the save.
                    reload.update(|value| *value += 1);
                }
                Err(server_error) => error.set(err_text(server_error)),
            }
            busy.set(false);
        });
    };

    let subject_button = move |target: PropertySubject| {
        view! {
            <button
                type="button"
                on:click=move |_| switch_subject(target)
                class=move || {
                    if subject.get() == target {
                        "rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white"
                    } else {
                        "rounded-full border border-slate-700 px-4 py-2 text-sm font-medium text-slate-300 hover:bg-slate-800"
                    }
                }
            >
                {target.label()}
            </button>
        }
    };

    let candidate_rows = move || {
        if loading.get() {
            return view! { <p class="p-4 text-sm text-slate-500">"Loading\u{2026}"</p> }.into_any();
        }
        let rows = candidates.get();
        if rows.is_empty() {
            return view! {
                <p class="p-4 text-sm text-slate-500">"No records match these filters."</p>
            }
            .into_any();
        }
        rows.into_iter()
            .map(|candidate| {
                let id = candidate.id.clone();
                let checked_id = candidate.id.clone();
                let name = candidate.name.clone();
                let subtitle = candidate.subtitle.clone();
                let has_subtitle = !subtitle.is_empty();
                let archived = candidate.archived;
                let current = candidate.current_value.clone();
                view! {
                    <label class="flex cursor-pointer items-start gap-3 border-b border-slate-800 p-3 last:border-b-0 hover:bg-slate-900">
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
                                preview.set(None);
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
                            <span class="block text-sm font-medium text-slate-100">
                                {name}
                                <Show when=move || archived>
                                    <span class="ml-2 rounded-full bg-slate-700/40 px-2 py-0.5 text-xs text-slate-300">
                                        "Archived"
                                    </span>
                                </Show>
                            </span>
                            <Show when=move || has_subtitle>
                                <span class="block text-xs text-slate-500">{subtitle.clone()}</span>
                            </Show>
                            <span class="mt-1 block text-xs">
                                {move || {
                                    if !has_applied_target.get() {
                                        return ().into_any();
                                    }
                                    match current.clone() {
                                        None => view! {
                                            <span class="text-slate-600">"Property not set"</span>
                                        }.into_any(),
                                        Some(existing) if existing.is_empty() => view! {
                                            <span class="text-slate-600">"Currently empty"</span>
                                        }.into_any(),
                                        Some(existing) => view! {
                                            <span class="text-slate-400">"Currently: " {existing}</span>
                                        }.into_any(),
                                    }
                                }}
                            </span>
                        </span>
                    </label>
                }
            })
            .collect_view()
            .into_any()
    };

    let property_datalist = move || {
        key_options
            .get()
            .into_iter()
            .map(|option| {
                let label = option.label();
                view! { <option value=option.key.clone()>{label}</option> }
            })
            .collect_view()
    };

    let section_datalist = move || {
        let mut sections: Vec<String> = key_options
            .get()
            .into_iter()
            .map(|option| option.section)
            .filter(|section| !section.is_empty())
            .collect();
        sections.sort();
        sections.dedup();
        sections
            .into_iter()
            .map(|section| view! { <option value=section.clone()></option> })
            .collect_view()
    };

    view! {
        <div class="space-y-5">
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div>
                    <p class="max-w-3xl text-sm text-slate-400">
                        "Name one property, find the records that need it, and set the same value on all of them at once."
                    </p>
                    <p class="mt-1 text-xs text-slate-500">
                        "A record that already has the property keeps its position in its own list; only the value changes. Records that do not have it get it added at the end."
                    </p>
                </div>
                <A
                    href=move || {
                        match subject.get() {
                            PropertySubject::Contact => "/contacts".to_string(),
                            PropertySubject::Organization => "/organizations".to_string(),
                        }
                    }
                    attr:class="rounded-full border border-slate-700 px-3 py-2 text-sm text-slate-300 hover:bg-slate-800"
                >
                    "Back"
                </A>
            </div>

            <Show when=move || !error.get().is_empty()>
                <p role="alert" class="rounded-lg border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-sm text-rose-300">
                    {move || error.get()}
                </p>
            </Show>
            <Show when=move || !notice.get().is_empty()>
                <p role="status" class="rounded-lg border border-emerald-500/40 bg-emerald-500/10 px-3 py-2 text-sm text-emerald-300">
                    {move || notice.get()}
                </p>
            </Show>

            <section class=PANEL>
                <h2 class="text-lg font-semibold text-slate-100">"1. Choose the property"</h2>
                <div class="mt-3 flex flex-wrap gap-2">
                    {PropertySubject::ALL.iter().map(|target| subject_button(*target)).collect_view()}
                </div>
                <div class="mt-4 grid gap-3 md:grid-cols-3">
                    <label>
                        <span class=LABEL>"Section"</span>
                        <input
                            class=INPUT
                            list="bulk-property-sections"
                            placeholder="General"
                            prop:value=move || section.get()
                            on:input=move |event| section.set(event_target_value(&event))
                        />
                        <datalist id="bulk-property-sections">{section_datalist}</datalist>
                    </label>
                    <label>
                        <span class=LABEL>"Property name *"</span>
                        <input
                            class=INPUT
                            list="bulk-property-keys"
                            placeholder="Relationship status"
                            prop:value=move || key.get()
                            on:input=move |event| key.set(event_target_value(&event))
                        />
                        <datalist id="bulk-property-keys">{property_datalist}</datalist>
                    </label>
                    <label>
                        <span class=LABEL>"Value to set"</span>
                        <input
                            class=INPUT
                            placeholder="Active"
                            prop:value=move || value.get()
                            on:input=move |event| value.set(event_target_value(&event))
                        />
                    </label>
                </div>
                <p class="mt-2 text-xs text-slate-500">
                    "Leaving the value empty is allowed: it names the field without answering it yet, the same as a blank row on a record."
                </p>
            </section>

            <section class=PANEL>
                <div class="flex flex-wrap items-start justify-between gap-3">
                    <div>
                        <h2 class="text-lg font-semibold text-slate-100">"2. Find and select records"</h2>
                        <p class="mt-1 text-sm text-slate-500">
                            "Filters apply when you press Apply filters, and the list shows each record's current value for the property above."
                        </p>
                    </div>
                    <span class="rounded-full bg-primary-500/15 px-3 py-1 text-sm font-semibold text-primary-300">
                        {move || selected_count.get()} " selected"
                    </span>
                </div>

                <div class="mt-4 grid gap-3 md:grid-cols-3">
                    <label>
                        <span class=LABEL>"Search"</span>
                        <input
                            class=INPUT
                            placeholder="Name, email, or organization"
                            prop:value=move || keyword.get()
                            on:input=move |event| keyword.set(event_target_value(&event))
                        />
                    </label>
                    <Show when=move || subject.get() == PropertySubject::Contact>
                        <label>
                            <span class=LABEL>"Contact type"</span>
                            <select
                                class=INPUT
                                prop:value=move || contact_type.get()
                                on:change=move |event| contact_type.set(event_target_value(&event))
                            >
                                <option value="">"Any type"</option>
                                {ContactType::ALL.iter().map(|kind| view! {
                                    <option value=kind.slug()>{kind.label()}</option>
                                }).collect_view()}
                            </select>
                        </label>
                        <label>
                            <span class=LABEL>"Organization"</span>
                            <select
                                class=INPUT
                                prop:value=move || organization_id.get()
                                on:change=move |event| organization_id.set(event_target_value(&event))
                            >
                                <option value="">"Any organization"</option>
                                {move || organizations.get().into_iter().map(|(id, name)| view! {
                                    <option value=id>{name}</option>
                                }).collect_view()}
                            </select>
                        </label>
                    </Show>
                    <Show when=move || subject.get() == PropertySubject::Organization>
                        <label>
                            <span class=LABEL>"Organization kind"</span>
                            <select
                                class=INPUT
                                prop:value=move || organization_kind.get()
                                on:change=move |event| organization_kind.set(event_target_value(&event))
                            >
                                <option value="">"Any kind"</option>
                                {OrganizationKind::ALL.iter().map(|kind| view! {
                                    <option value=kind.slug()>{kind.label()}</option>
                                }).collect_view()}
                            </select>
                        </label>
                    </Show>
                </div>

                <label class="mt-3 flex cursor-pointer items-start gap-2 text-sm text-slate-300">
                    <input
                        type="checkbox"
                        class="mt-0.5 h-4 w-4 accent-primary-500"
                        prop:checked=move || only_missing_target.get()
                        on:change=move |event| only_missing_target.set(event_target_checked(&event))
                    />
                    <span>
                        "Only records that do not have this property yet"
                        <span class="block text-xs text-slate-500">
                            "The backfill case: everyone the property has never been set on. The chips above pick records by the values they already carry, which is a different question."
                        </span>
                    </span>
                </label>

                <label class="mt-3 flex cursor-pointer items-center gap-2 text-sm text-slate-300">
                    <input
                        type="checkbox"
                        class="h-4 w-4 accent-primary-500"
                        prop:checked=move || include_archived.get()
                        on:change=move |event| include_archived.set(event_target_checked(&event))
                    />
                    "Include archived records"
                </label>

                <div class="mt-4">
                    <span class=LABEL>"Filter by properties they already have"</span>
                    // Remounted per subject: the facets come from a different
                    // table, and switching subjects clears the chips anyway.
                    {move || view! {
                        <PropertyFilterBar
                            subject=subject.get()
                            filters=property_filters
                            scope=facet_scope
                        />
                    }}
                </div>

                <button
                    type="button"
                    on:click=apply_filters
                    class="mt-3 rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                >
                    "Apply filters"
                </button>

                <div class="mt-5 flex flex-wrap items-center justify-between gap-2 text-xs text-slate-500">
                    <span>
                        "Showing " {move || candidates.get().len()} " of " {move || total.get()}
                        " " {move || subject.get().noun_plural()}
                    </span>
                    <div class="flex flex-wrap gap-2">
                        <button
                            type="button"
                            prop:disabled=move || loading.get()
                            on:click=move |_| {
                                let ids = candidates.get_untracked().into_iter().map(|c| c.id).collect::<Vec<_>>();
                                preview.set(None);
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
                            }
                            class="text-primary-300 hover:text-primary-200 disabled:cursor-not-allowed disabled:opacity-40"
                        >
                            "Select visible"
                        </button>
                        <button
                            type="button"
                            prop:disabled=move || loading.get()
                            on:click=move |_| {
                                preview.set(None);
                                all_matching.set(true);
                                selected_ids.set(Vec::new());
                                excluded_ids.set(Vec::new());
                            }
                            class="text-primary-300 hover:text-primary-200 disabled:cursor-not-allowed disabled:opacity-40"
                        >
                            "Select all matching"
                        </button>
                        <button
                            type="button"
                            on:click=move |_| clear_selection()
                            class="text-slate-300 hover:text-white"
                        >
                            "Clear selection"
                        </button>
                    </div>
                </div>

                <Show when=move || all_matching.get()>
                    <p class="mt-2 text-xs text-primary-300">
                        "Every matching record is selected except " {move || excluded_ids.get().len()} "."
                    </p>
                </Show>

                <div class="mt-3 overflow-hidden rounded-lg border border-slate-800 bg-slate-950">
                    {candidate_rows}
                </div>

                <div class="mt-3 flex justify-between gap-3">
                    <button
                        type="button"
                        prop:disabled=move || offset.get() == 0
                        on:click=move |_| offset.update(|value| *value = (*value - CANDIDATE_PAGE_SIZE).max(0))
                        class="rounded-full border border-slate-700 px-3 py-1.5 text-sm text-slate-300 disabled:opacity-40"
                    >
                        "Previous"
                    </button>
                    <button
                        type="button"
                        prop:disabled=move || on_last_page.get()
                        on:click=move |_| offset.update(|value| *value += CANDIDATE_PAGE_SIZE)
                        class="rounded-full border border-slate-700 px-3 py-1.5 text-sm text-slate-300 disabled:opacity-40"
                    >
                        "Next"
                    </button>
                </div>
            </section>

            <section class=PANEL>
                <h2 class="text-lg font-semibold text-slate-100">"3. Review and save"</h2>
                <p class="mt-1 text-sm text-slate-500">
                    "Check what the save would change before committing it."
                </p>

                <Show when=move || preview.get().is_some()>
                    {move || {
                        let Some(result) = preview.get() else {
                            return ().into_any();
                        };
                        let at_limit = result.at_property_limit;
                        view! {
                            <dl class="mt-4 grid gap-3 sm:grid-cols-4">
                                <PreviewTile label="Selected" value=result.total />
                                <PreviewTile label="Will be added" value=result.will_add />
                                <PreviewTile label="Will be overwritten" value=result.will_overwrite />
                                <PreviewTile label="Already correct" value=result.unchanged />
                            </dl>
                            {(at_limit != 0).then(|| view! {
                                <p class="mt-3 text-sm text-amber-300">
                                    {plural(at_limit, "record", "records")}
                                    " already hold the maximum number of properties and will be skipped."
                                </p>
                            })}
                        }
                        .into_any()
                    }}
                </Show>

                <Show when=move || outcome.get().is_some()>
                    {move || {
                        let Some(result) = outcome.get() else {
                            return ().into_any();
                        };
                        let skipped = result.skipped.clone();
                        view! {
                            <dl class="mt-4 grid gap-3 sm:grid-cols-3">
                                <PreviewTile label="Added" value=result.added />
                                <PreviewTile label="Overwritten" value=result.overwritten />
                                <PreviewTile label="Unchanged" value=result.unchanged />
                            </dl>
                            {(!skipped.is_empty()).then(|| view! {
                                <p class="mt-3 text-sm text-amber-300">
                                    "Skipped (at the property limit): "
                                    {skipped.join(", ")}
                                </p>
                            })}
                        }
                        .into_any()
                    }}
                </Show>

                <div class="mt-4 flex flex-wrap gap-2">
                    <button
                        type="button"
                        on:click=run_preview
                        prop:disabled=move || busy.get()
                        class="rounded-full border border-slate-700 px-4 py-2 text-sm font-medium text-slate-200 hover:bg-slate-800 disabled:opacity-50"
                    >
                        {move || if busy.get() { "Working\u{2026}" } else { "Preview changes" }}
                    </button>
                    <button
                        type="button"
                        on:click=apply_edit
                        prop:disabled=move || busy.get()
                        class="rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                    >
                        {move || {
                            if busy.get() {
                                "Saving\u{2026}".to_string()
                            } else {
                                format!("Save to {}", plural(selected_count.get(), "record", "records"))
                            }
                        }}
                    </button>
                </div>
            </section>
        </div>
    }
}

#[component]
fn PreviewTile(label: &'static str, value: i64) -> impl IntoView {
    view! {
        <div class="rounded-lg border border-slate-800 bg-slate-950 p-3">
            <dt class="text-xs text-slate-500">{label}</dt>
            <dd class="mt-1 text-lg font-semibold text-slate-100">{value}</dd>
        </div>
    }
}
