//! The organization directory and detail views.
//!
//! The top-level page owns the information-management guard. Every permitted
//! non-client account may create, edit, archive, and connect organization data.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map, use_query_map};
use leptos_router::NavigateOptions;

use crate::components::change_log::ChangeLog;
use crate::components::contact_form::ContactForm;
use crate::components::guard::require_information_management_access;
use crate::components::layout::Layout;
use crate::components::loading::Loading;
use crate::components::organization_properties::OrganizationPropertiesPanel;
use crate::components::property_filters::{MatchedProperties, PropertyFilterBar};
use crate::helpers::format::badge_pill;
use crate::pages::contacts::encode_query_value;
use crate::server_fns::audit::AuditScope;
use crate::server_fns::contacts::{
    list_contacts, search_active_contacts, set_contact_organization, ActiveContactSummary, Contact,
    ContactFilters,
};
use crate::server_fns::err_text;
use crate::server_fns::grants::{list_grants, Grant, GrantFilters};
use crate::server_fns::organizations::{
    create_organization, list_organizations, load_organization, set_organization_archived,
    update_organization, Organization, OrganizationFilters, OrganizationInput, OrganizationKind,
};
use crate::server_fns::property_filters::{
    self, PropertyFacetScope, PropertyFilter, PropertySubject,
};
use crate::state::AppState;

const PANEL: &str = "rounded-card border border-slate-800 bg-slate-900 shadow-card p-5";
const INPUT: &str =
    "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-600 focus:border-primary-500 focus:outline-none";
const LABEL: &str = "text-xs font-medium text-slate-400";

/// Canonical top-level Organizations list/detail page.
#[component]
pub fn OrganizationsPage() -> impl IntoView {
    let state = expect_context::<AppState>();
    let params = use_params_map();
    require_information_management_access(state, move || {
        let selected_id = params.read().get("id").filter(|id| !id.trim().is_empty());
        view! {
            <Layout title="Organizations".to_string()>
                <ManageOrganizations selected_id=selected_id />
            </Layout>
        }
        .into_any()
    })
}

/// The Organizations workspace: the directory, or one organization.
#[component]
pub fn ManageOrganizations(selected_id: Option<String>) -> impl IntoView {
    match selected_id {
        Some(id) => view! { <OrganizationDetail organization_id=id /> }.into_any(),
        None => view! { <OrganizationDirectory /> }.into_any(),
    }
}

#[component]
fn OrganizationDirectory() -> impl IntoView {
    let state = expect_context::<AppState>();
    let can_manage = state.has_information_management_access();
    // Importing writes records wholesale, so it needs the admin gate too.
    let can_import =
        state.has_operations_admin_permissions() && state.has_information_management_access();

    // Seed the filters from the query string, so a shared link opens the view
    // its sender was looking at.
    let initial = use_query_map().get_untracked();
    let initial_keyword = initial.get("q").unwrap_or_default();
    let initial_kind = initial.get("kind").unwrap_or_default();
    let initial_archived = initial.get("archived").as_deref() == Some("1");
    let initial_properties = property_filters::decode(&initial.get("props").unwrap_or_default());
    drop(initial);

    let keyword = RwSignal::new(initial_keyword.clone());
    let kind_filter = RwSignal::new(initial_kind.clone());
    let include_archived = RwSignal::new(initial_archived);
    let property_filters = RwSignal::new(initial_properties.clone());
    let applied = RwSignal::new(OrganizationFilters {
        keyword: initial_keyword,
        kind: OrganizationKind::from_slug(&initial_kind),
        include_archived: initial_archived,
        property_filters: initial_properties,
    });
    let items = RwSignal::new(Vec::<Organization>::new());
    let total = RwSignal::new(0i64);
    let window = RwSignal::new(50i64);
    let loading = RwSignal::new(true);
    let error = RwSignal::new(String::new());
    let reload = RwSignal::new(0u32);
    let creating = RwSignal::new(false);

    // Facet counts are measured against the filters actually applied, not the
    // half-typed ones sitting in the form above the Apply button.
    let facet_scope = Signal::derive(move || {
        let applied = applied.get();
        PropertyFacetScope {
            keyword: applied.keyword,
            include_archived: applied.include_archived,
            organization_kind: applied
                .kind
                .map(|kind| kind.slug().to_string())
                .unwrap_or_default(),
            ..Default::default()
        }
    });

    Effect::new(move |_| {
        let filters = applied.get();
        let limit = window.get();
        reload.track();
        loading.set(true);
        spawn_local(async move {
            match list_organizations(filters, 0, limit).await {
                Ok(page) => {
                    items.set(page.items);
                    total.set(page.total);
                    error.set(String::new());
                }
                Err(e) => error.set(err_text(e)),
            }
            loading.set(false);
        });
    });

    // Chips commit on their own Apply, so they take effect without waiting for
    // the keyword form's button; folding them into `applied` keeps that button
    // from dropping them.
    Effect::new(move |previous: Option<Vec<PropertyFilter>>| {
        let current = property_filters.get();
        if previous.is_some_and(|previous| previous != current) {
            window.set(50);
            applied.update(|applied| applied.property_filters = current.clone());
        }
        current
    });

    // Mirror the applied filters into the URL so the view can be linked to.
    let navigate = use_navigate();
    Effect::new(move |previous: Option<()>| {
        let current = applied.get();
        let mut parts = Vec::<String>::new();
        if !current.keyword.is_empty() {
            parts.push(format!("q={}", encode_query_value(&current.keyword)));
        }
        if let Some(kind) = current.kind {
            parts.push(format!("kind={}", kind.slug()));
        }
        let props = property_filters::encode(&current.property_filters);
        if !props.is_empty() {
            parts.push(format!("props={}", encode_query_value(&props)));
        }
        if current.include_archived {
            parts.push("archived=1".to_string());
        }
        // Skip the first run: it would only rewrite the URL we just read.
        if previous.is_some() {
            let target = if parts.is_empty() {
                "/organizations".to_string()
            } else {
                format!("/organizations?{}", parts.join("&"))
            };
            navigate(
                &target,
                NavigateOptions {
                    replace: true,
                    scroll: false,
                    ..Default::default()
                },
            );
        }
    });

    let apply = move |_| {
        window.set(50);
        applied.set(OrganizationFilters {
            keyword: keyword.get_untracked(),
            kind: OrganizationKind::from_slug(&kind_filter.get_untracked()),
            include_archived: include_archived.get_untracked(),
            property_filters: property_filters.get_untracked(),
        });
    };

    let rows = move || {
        if !error.get().is_empty() {
            return view! { <p class="text-sm text-rose-300">{error.get()}</p> }.into_any();
        }
        let list = items.get();
        if list.is_empty() {
            let message = if loading.get() {
                "Loading organizations\u{2026}"
            } else {
                "No organizations match these filters."
            };
            return view! { <p class="text-sm text-slate-500">{message}</p> }.into_any();
        }
        list.into_iter()
            .map(|org| {
                let href = format!("/organizations/{}", org.id);
                let kind = org.kind;
                let archived = org.archived;
                let count = org.contact_count;
                let matched = org.filtered_properties.clone();
                view! {
                    <A href=href attr:class="block rounded-lg border border-slate-800 bg-slate-950 p-3 hover:border-primary-500/40">
                        <div class="flex flex-wrap items-center gap-2">
                            <span class="text-sm font-semibold text-slate-100">{org.name.clone()}</span>
                            <span class=badge_pill(kind.badge_classes())>{kind.label()}</span>
                            <Show when=move || archived>
                                <span class=badge_pill("bg-slate-700/40 text-slate-300 ring-1 ring-slate-600")>"Archived"</span>
                            </Show>
                        </div>
                        <p class="mt-1 text-xs text-slate-500">
                            {count} {if count == 1 { " contact" } else { " contacts" }}
                            {if org.email.is_empty() { String::new() } else { format!(" \u{b7} {}", org.email) }}
                        </p>
                        <div class="mt-2 flex flex-wrap gap-1">
                            <MatchedProperties properties=matched />
                        </div>
                    </A>
                }
            })
            .collect_view()
            .into_any()
    };

    view! {
        <div class=PANEL>
            <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                <div>
                    <h2 class="text-lg font-semibold text-slate-100">"Organizations"</h2>
                    <p class="mt-1 text-sm text-slate-500">
                        "Funders, partner agencies, providers, courts, and employers."
                    </p>
                </div>
                <Show when=move || can_manage>
                    <div class="flex shrink-0 flex-wrap gap-2">
                        <A
                            href="/properties/bulk?subject=organization"
                            attr:class="rounded-full border border-slate-700 px-4 py-2 text-sm font-semibold text-slate-200 hover:bg-slate-800"
                        >
                            "Bulk edit properties"
                        </A>
                        <Show when=move || can_import>
                            <A
                                href="/import?subject=organization"
                                attr:class="rounded-full border border-slate-700 px-4 py-2 text-sm font-semibold text-slate-200 hover:bg-slate-800"
                            >
                                "Import from file"
                            </A>
                        </Show>
                        <button
                            type="button"
                            on:click=move |_| creating.update(|c| *c = !*c)
                            class="shrink-0 rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600"
                        >
                            {move || if creating.get() { "Cancel" } else { "+ New organization" }}
                        </button>
                    </div>
                </Show>
            </div>

            <Show when=move || can_manage && creating.get()>
                <div class="mt-4 border-t border-slate-800 pt-4">
                    <OrganizationForm
                        organization=None
                        on_saved=Callback::new(move |_id: String| {
                            creating.set(false);
                            reload.update(|r| *r += 1);
                        })
                    />
                </div>
            </Show>

            <div class="mt-4 grid gap-3 sm:grid-cols-3">
                <label class="block">
                    <span class=LABEL>"Search"</span>
                    <input
                        class=INPUT
                        placeholder="Name or email"
                        prop:value=move || keyword.get()
                        on:input=move |e| keyword.set(event_target_value(&e))
                    />
                </label>
                <label class="block">
                    <span class=LABEL>"Type"</span>
                    <select
                        class=INPUT
                        prop:value=move || kind_filter.get()
                        on:change=move |e| kind_filter.set(event_target_value(&e))
                    >
                        <option value="">"Any type"</option>
                        {OrganizationKind::ALL
                            .iter()
                            .map(|k| view! { <option value=k.slug()>{k.label()}</option> })
                            .collect_view()}
                    </select>
                </label>
                <label class="flex items-end gap-2 pb-2 text-sm text-slate-300">
                    <input
                        type="checkbox"
                        class="h-4 w-4 rounded border-slate-700 bg-slate-950"
                        prop:checked=move || include_archived.get()
                        on:change=move |e| include_archived.set(event_target_checked(&e))
                    />
                    "Include archived"
                </label>
            </div>
            <button
                type="button"
                on:click=apply
                class="mt-3 rounded-full bg-primary-500 px-3 py-1.5 text-sm font-semibold text-white hover:bg-primary-600"
            >
                "Apply filters"
            </button>

            <div class="mt-4">
                <PropertyFilterBar
                    subject=PropertySubject::Organization
                    filters=property_filters
                    scope=facet_scope
                />
            </div>

            <div class="mt-4 space-y-2">{rows}</div>
            <div class="mt-4 flex items-center justify-between text-xs text-slate-500">
                <span>"Showing " {move || items.get().len()} " of " {move || total.get()} " organizations"</span>
                <Show when=move || (items.get().len() as i64) < total.get()>
                    <button type="button" on:click=move |_| window.update(|value| *value += 50)
                        class="rounded-full border border-slate-700 px-3 py-1.5 text-sm text-slate-200 hover:bg-slate-800">
                        "Load more"
                    </button>
                </Show>
            </div>
        </div>
    }
}

#[component]
fn OrganizationDetail(organization_id: String) -> impl IntoView {
    let state = expect_context::<AppState>();
    let can_manage = state.has_information_management_access();
    let id = StoredValue::new(organization_id);

    let organization = RwSignal::new(None::<Organization>);
    let contacts = RwSignal::new(Vec::<Contact>::new());
    let grants = RwSignal::new(Vec::<Grant>::new());
    let contact_total = RwSignal::new(0i64);
    let contact_window = RwSignal::new(50i64);
    let grant_total = RwSignal::new(0i64);
    let grant_window = RwSignal::new(50i64);
    let loading = RwSignal::new(true);
    let error = RwSignal::new(String::new());
    let editing = RwSignal::new(false);
    let reload = RwSignal::new(0u32);

    Effect::new(move |_| {
        reload.track();
        let contact_limit = contact_window.get();
        let grant_limit = grant_window.get();
        loading.set(true);
        spawn_local(async move {
            match load_organization(id.get_value()).await {
                Ok(found) => {
                    organization.set(found);
                    error.set(String::new());
                }
                Err(e) => error.set(err_text(e)),
            }
            let filters = ContactFilters {
                organization_id: id.get_value(),
                include_archived: true,
                ..Default::default()
            };
            if let Ok(page) = list_contacts(filters, 0, contact_limit).await {
                contacts.set(page.items);
                contact_total.set(page.total);
            }
            if can_manage {
                let filters = GrantFilters {
                    funder_organization_id: id.get_value(),
                    ..Default::default()
                };
                if let Ok(page) = list_grants(filters, 0, grant_limit).await {
                    grants.set(page.items);
                    grant_total.set(page.total);
                }
            }
            loading.set(false);
        });
    });

    let toggle_archive = move |_| {
        let Some(current) = organization.get_untracked() else {
            return;
        };
        let next = !current.archived;
        spawn_local(async move {
            match set_organization_archived(id.get_value(), next).await {
                Ok(()) => reload.update(|r| *r += 1),
                Err(e) => error.set(err_text(e)),
            }
        });
    };

    view! {
        <div class="space-y-6">
            <Show when=move || loading.get() && organization.get().is_none()>
                <div class=PANEL><Loading label="Loading organization\u{2026}" /></div>
            </Show>
            <Show when=move || !error.get().is_empty()>
                <p class="text-sm text-rose-300">{move || error.get()}</p>
            </Show>

            {move || {
                let Some(org) = organization.get() else {
                    return ().into_any();
                };
                let kind = org.kind;
                let archived = org.archived;
                let detail = org.clone();
                view! {
                    <div class=PANEL>
                        <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                            <div class="flex flex-wrap items-center gap-2">
                                <h2 class="text-lg font-semibold text-slate-100">{org.name.clone()}</h2>
                                <span class=badge_pill(kind.badge_classes())>{kind.label()}</span>
                                <Show when=move || archived>
                                    <span class=badge_pill("bg-slate-700/40 text-slate-300 ring-1 ring-slate-600")>"Archived"</span>
                                </Show>
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
                            fallback=move || {
                                let o = detail.clone();
                                view! {
                                    <dl class="mt-4 grid gap-x-6 sm:grid-cols-2">
                                        <OrgRow label="Website" value=o.website.clone() />
                                        <OrgRow label="Email" value=o.email.clone() />
                                        <OrgRow label="Phone" value=o.phone.clone() />
                                        <OrgRow label="Address" value=o.address.clone() />
                                        <OrgRow label="Notes" value=o.description.clone() />
                                    </dl>
                                }
                            }
                        >
                            <div class="mt-4 border-t border-slate-800 pt-4">
                                <OrganizationForm
                                    organization=organization.get()
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

            <Show when=move || can_manage>
                <OrganizationPropertiesPanel organization_id=id.get_value() />
            </Show>

            {move || organization.get().map(|org| view! {
                <OrganizationPeoplePanel
                    organization_id=id.get_value()
                    organization_name=org.name
                    contacts
                    total=contact_total
                    window=contact_window
                    reload
                    can_manage=can_manage
                />
            })}

            <Show when=move || can_manage>
                <div class=PANEL>
                    <h3 class="text-sm font-semibold text-slate-200">"Grants"</h3>
                    <div class="mt-3 space-y-2">
                        {move || {
                            let list = grants.get();
                            if list.is_empty() {
                                return view! { <p class="text-sm text-slate-500">"No grants are linked to this organization."</p> }.into_any();
                            }
                            list.into_iter().map(|grant| {
                                let href = format!("/funding/{}", grant.id);
                                let status = grant.status;
                                view! {
                                    <A href=href attr:class="block rounded-lg border border-slate-800 bg-slate-950 p-3 hover:border-primary-500/40">
                                        <span class="text-sm font-medium text-slate-100">{grant.name}</span>
                                        <span class=badge_pill(status.badge_classes())>{status.label()}</span>
                                    </A>
                                }
                            }).collect_view().into_any()
                        }}
                    </div>
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
                    <h3 class="text-sm font-semibold text-slate-200">"Change log"</h3>
                    <div class="mt-3">
                        <ChangeLog scope=AuditScope::Organization entity_id=id.get_value() />
                    </div>
                </div>
            </Show>

            <A href="/organizations" attr:class="inline-block text-sm text-primary-400 hover:text-primary-300">
                "\u{2190} Back to organizations"
            </A>
        </div>
    }
}

#[component]
fn OrganizationPeoplePanel(
    organization_id: String,
    organization_name: String,
    contacts: RwSignal<Vec<Contact>>,
    total: RwSignal<i64>,
    window: RwSignal<i64>,
    reload: RwSignal<u32>,
    can_manage: bool,
) -> impl IntoView {
    let id = StoredValue::new(organization_id);
    let name = StoredValue::new(organization_name);
    let creating = RwSignal::new(false);
    let linking = RwSignal::new(false);
    let query = RwSignal::new(String::new());
    let debounced_query = RwSignal::new(String::new());
    let debounce_generation = RwSignal::new(0u64);
    let results = RwSignal::new(Vec::<ActiveContactSummary>::new());
    let error = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let organizations = RwSignal::new(Vec::<(String, String)>::new());
    organizations.set(vec![(id.get_value(), name.get_value())]);

    Effect::new(move |_| {
        if !linking.get() {
            return;
        }
        let q = debounced_query.get();
        spawn_local(async move {
            match search_active_contacts(q).await {
                Ok(list) => results.set(list),
                Err(e) => error.set(err_text(e)),
            }
        });
    });

    let rows = move || {
        let list = contacts.get();
        if list.is_empty() {
            return view! { <p class="text-sm text-slate-500">"No people are filed under this organization."</p> }.into_any();
        }
        list.into_iter().map(|person| {
            let href = format!("/contacts/{}", person.id);
            let person_id = person.id.clone();
            let title = person.job_title.clone();
            let archived = person.archived;
            let unlink = move |_| {
                busy.set(true);
                let person_id = person_id.clone();
                spawn_local(async move {
                    match set_contact_organization(person_id, String::new()).await {
                        Ok(()) => reload.update(|value| *value += 1),
                        Err(e) => error.set(err_text(e)),
                    }
                    busy.set(false);
                });
            };
            view! {
                <div class="flex items-center justify-between gap-3 rounded-lg border border-slate-800 bg-slate-950 p-3">
                    <div>
                        <A href=href attr:class="text-sm font-medium text-slate-100 hover:text-primary-300">{person.display_name()}</A>
                        {(!title.is_empty()).then(|| view! { <span class="ml-2 text-xs text-slate-500">{title}</span> })}
                        {archived.then(|| view! { <span class=badge_pill("bg-slate-700/40 text-slate-300 ring-1 ring-slate-600")>"Archived"</span> })}
                    </div>
                    {if can_manage {
                        view! {
                        <button type="button" on:click=unlink prop:disabled=move || busy.get()
                            class="rounded-full border border-slate-700 px-2 py-1 text-xs text-slate-400 hover:bg-slate-800 disabled:opacity-50">
                            "Remove"
                        </button>
                        }.into_any()
                    } else {
                        ().into_any()
                    }}
                </div>
            }
        }).collect_view().into_any()
    };

    Effect::new(move |_| {
        let value = query.get();
        debounce_generation.update(|generation| *generation += 1);
        let generation = debounce_generation.get_untracked();
        set_timeout(
            move || {
                if debounce_generation.get_untracked() == generation {
                    debounced_query.set(value);
                }
            },
            std::time::Duration::from_millis(300),
        );
    });

    view! {
        <div class=PANEL>
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div>
                    <h3 class="text-sm font-semibold text-slate-200">"People here"</h3>
                    <p class="mt-1 text-xs text-slate-500">"Create a person here or file an existing person under this organization."</p>
                </div>
                {if can_manage {
                    view! { <div class="flex gap-2">
                        <button type="button" on:click=move |_| { creating.update(|value| *value = !*value); linking.set(false); }
                            class="rounded-full border border-slate-700 px-3 py-1.5 text-xs font-medium text-slate-200 hover:bg-slate-800">
                            {move || if creating.get() { "Cancel" } else { "+ New person here" }}
                        </button>
                        <button type="button" on:click=move |_| { linking.update(|value| *value = !*value); creating.set(false); }
                            class="rounded-full bg-primary-500 px-3 py-1.5 text-xs font-semibold text-white hover:bg-primary-600">
                            {move || if linking.get() { "Cancel" } else { "File existing person" }}
                        </button>
                    </div> }.into_any()
                } else {
                    ().into_any()
                }}
            </div>

            <Show when=move || can_manage && creating.get()>
                <div class="mt-4 border-t border-slate-800 pt-4">
                    <ContactForm
                        contact=None
                        organizations
                        initial_organization_id=id.get_value()
                        on_saved=Callback::new(move |_: String| {
                            creating.set(false);
                            reload.update(|value| *value += 1);
                        })
                    />
                </div>
            </Show>

            <Show when=move || can_manage && linking.get()>
                <div class="mt-4 space-y-2 border-t border-slate-800 pt-4">
                    <label class="block">
                        <span class=LABEL>"Search active people"</span>
                        <input type="search" class=INPUT placeholder="Name, email, phone, or organization"
                            prop:value=move || query.get()
                            on:input=move |event| {
                                query.set(event_target_value(&event));
                            } />
                    </label>
                    {move || results.get().into_iter()
                        .filter(|person| person.organization_id != id.get_value())
                        .map(|person| {
                            let person_id = person.id.clone();
                            let moving = !person.organization_id.is_empty();
                            let current = person.organization_name.clone();
                            let label = person.label.clone();
                            let link = move |_| {
                                if moving && !current.is_empty() && !crate::pages::organizations::confirm_move(&label, &current, &name.get_value()) {
                                    return;
                                }
                                busy.set(true);
                                let person_id = person_id.clone();
                                spawn_local(async move {
                                    match set_contact_organization(person_id, id.get_value()).await {
                                        Ok(()) => {
                                            linking.set(false);
                                            reload.update(|value| *value += 1);
                                        }
                                        Err(e) => error.set(err_text(e)),
                                    }
                                    busy.set(false);
                                });
                            };
                            view! {
                                <div class="flex items-center justify-between gap-3 rounded-lg border border-slate-800 bg-slate-950 p-3">
                                    <div>
                                        <p class="text-sm text-slate-100">{person.label}</p>
                                        <p class="text-xs text-slate-500">{if moving { format!("Currently filed under {}", person.organization_name) } else { "No current organization".to_string() }}</p>
                                    </div>
                                    <button type="button" on:click=link prop:disabled=move || busy.get()
                                        class="rounded-full border border-primary-500/40 px-3 py-1.5 text-xs font-medium text-primary-200 hover:bg-primary-500/10 disabled:opacity-50">
                                        {if moving { "Move here" } else { "File here" }}
                                    </button>
                                </div>
                            }
                        }).collect_view()}
                </div>
            </Show>

            <Show when=move || !error.get().is_empty()>
                <p class="mt-3 text-sm text-rose-300" role="alert">{move || error.get()}</p>
            </Show>
            <div class="mt-4 space-y-2">{rows}</div>
            <div class="mt-4 flex items-center justify-between text-xs text-slate-500">
                <span>"Showing " {move || contacts.get().len()} " of " {move || total.get()} " people"</span>
                <Show when=move || (contacts.get().len() as i64) < total.get()>
                    <button type="button" on:click=move |_| window.update(|value| *value += 50)
                        class="rounded-full border border-slate-700 px-3 py-1.5 text-sm text-slate-200 hover:bg-slate-800">
                        "Load more people"
                    </button>
                </Show>
            </div>
        </div>
    }
}

fn confirm_move(person: &str, current: &str, target: &str) -> bool {
    #[cfg(feature = "hydrate")]
    {
        let message = format!("Move {person} from {current} to {target}?");
        web_sys::window()
            .and_then(|window| window.confirm_with_message(&message).ok())
            .unwrap_or(false)
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = (person, current, target);
        false
    }
}

#[component]
fn OrgRow(label: &'static str, value: String) -> impl IntoView {
    let empty = value.trim().is_empty();
    view! {
        <div class="border-b border-slate-800 py-2">
            <dt class="text-xs font-medium text-slate-500">{label}</dt>
            <dd class=if empty {
                "mt-1 text-sm italic text-slate-500"
            } else {
                "mt-1 whitespace-pre-wrap text-sm text-slate-200"
            }>{if empty { "Not provided".to_string() } else { value }}</dd>
        </div>
    }
}

#[component]
fn OrganizationForm(
    organization: Option<Organization>,
    on_saved: Callback<String>,
) -> impl IntoView {
    let existing_id = organization.as_ref().map(|o| o.id.clone());
    let editing = StoredValue::new(existing_id.clone());
    let seed = organization.unwrap_or_default();

    let name = RwSignal::new(seed.name.clone());
    let kind = RwSignal::new(seed.kind.slug().to_string());
    let website = RwSignal::new(seed.website.clone());
    let phone = RwSignal::new(seed.phone.clone());
    let email = RwSignal::new(seed.email.clone());
    let address = RwSignal::new(seed.address.clone());
    let description = RwSignal::new(seed.description.clone());
    let error = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    let submit = move |_| {
        if busy.get_untracked() {
            return;
        }
        let input = OrganizationInput {
            name: name.get_untracked(),
            kind: OrganizationKind::from_slug(&kind.get_untracked()).unwrap_or_default(),
            website: website.get_untracked(),
            phone: phone.get_untracked(),
            email: email.get_untracked(),
            address: address.get_untracked(),
            description: description.get_untracked(),
        };
        if let Err(message) = input.validate() {
            error.set(message);
            return;
        }
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = match editing.get_value() {
                Some(id) => update_organization(id.clone(), input).await.map(|()| id),
                None => create_organization(input).await,
            };
            match result {
                Ok(id) => on_saved.run(id),
                Err(e) => error.set(err_text(e)),
            }
            busy.set(false);
        });
    };

    let field = move |label: &'static str, signal: RwSignal<String>| {
        view! {
            <label class="block">
                <span class=LABEL>{label}</span>
                <input
                    class=INPUT
                    prop:value=move || signal.get()
                    on:input=move |e| signal.set(event_target_value(&e))
                />
            </label>
        }
    };

    view! {
        <div class="space-y-4">
            <div class="grid gap-3 sm:grid-cols-2">
                {field("Name", name)}
                <label class="block">
                    <span class=LABEL>"Type"</span>
                    <select
                        class=INPUT
                        prop:value=move || kind.get()
                        on:change=move |e| kind.set(event_target_value(&e))
                    >
                        {OrganizationKind::ALL
                            .iter()
                            .map(|k| view! { <option value=k.slug()>{k.label()}</option> })
                            .collect_view()}
                    </select>
                </label>
                {field("Website", website)}
                {field("Email", email)}
                {field("Phone", phone)}
                {field("Address", address)}
            </div>
            <label class="block">
                <span class=LABEL>"Notes"</span>
                <textarea
                    class=INPUT
                    rows="3"
                    prop:value=move || description.get()
                    on:input=move |e| description.set(event_target_value(&e))
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
                        "Create organization"
                    }
                }}
            </button>
        </div>
    }
}
