//! The case-side view of case access: who is on a case, and a draft editor for
//! adding several volunteers at once or changing and removing their access.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;

use crate::server_fns::capabilities::{CaseCapability, CasePreset};
use crate::server_fns::case_team::CaseTeamMember;
use crate::server_fns::err_text;
use crate::server_fns::users::{AccountRole, UserSummary};
use crate::state::AppState;

/// One person in the team draft.
#[derive(Clone)]
struct DraftMember {
    user_id: String,
    name: String,
    role: AccountRole,
    caps: RwSignal<Vec<CaseCapability>>,
    removed: RwSignal<bool>,
}

fn same_caps(a: &[CaseCapability], b: &[CaseCapability]) -> bool {
    a.len() == b.len() && a.iter().all(|capability| b.contains(capability))
}

fn badge(classes: &str) -> String {
    format!("inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium {classes}")
}

/// The preset a capability set matches exactly, if any.
fn preset_label(caps: &[CaseCapability]) -> Option<&'static str> {
    CasePreset::ALL
        .into_iter()
        .find(|preset| same_caps(&preset.capabilities(), caps))
        .map(CasePreset::label)
}

#[component]
pub fn CaseTeam(
    case_id: String,
    is_site_admin: bool,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    let case_id = StoredValue::new(case_id);
    let actor_user_id = move || {
        state
            .current_user_summary
            .with_untracked(|user| user.as_ref().map(|user| user.id.clone()))
            .unwrap_or_default()
    };

    let team = RwSignal::new(Vec::<CaseTeamMember>::new());
    let loading = RwSignal::new(true);
    let load_error = RwSignal::new(None::<String>);
    let team_generation = RwSignal::new(0u64);

    Effect::new(move |_| {
        reload.track();
        if !state.has_operations_admin_permissions() {
            return;
        }
        loading.set(true);
        team_generation.update(|generation| *generation += 1);
        let generation = team_generation.get_untracked();
        spawn_local(async move {
            let response = crate::server_fns::case_team::load_case_team(case_id.get_value()).await;
            if team_generation.get_untracked() != generation {
                return;
            }
            match response {
                Ok(members) => {
                    team.set(members);
                    load_error.set(None);
                }
                Err(error) => load_error.set(Some(err_text(error))),
            }
            loading.set(false);
        });
    });

    // --- draft --------------------------------------------------------------
    let owner = StoredValue::new(Owner::current().expect("component owner"));
    let make_draft = move |user_id: String, name: String, role: AccountRole, caps: Vec<CaseCapability>| {
        owner.with_value(|current_owner| {
            current_owner.with(|| DraftMember {
                user_id,
                name,
                role,
                caps: RwSignal::new(caps),
                removed: RwSignal::new(false),
            })
        })
    };
    let editing = RwSignal::new(false);
    let draft = RwSignal::new(Vec::<DraftMember>::new());
    let saving = RwSignal::new(false);
    let feedback = RwSignal::new(None::<Result<String, String>>);
    let note = RwSignal::new(String::new());

    // --- people picker (multi-select) ---------------------------------------
    let search = RwSignal::new(String::new());
    let candidates = RwSignal::new(Vec::<UserSummary>::new());
    let candidates_loading = RwSignal::new(false);
    let selected = RwSignal::new(Vec::<String>::new());
    let preset = RwSignal::new(CasePreset::Contributor.slug().to_string());
    let search_generation = RwSignal::new(0u64);
    let search_id = StoredValue::new(format!("case-team-search-{}", case_id.get_value()));
    let preset_id = StoredValue::new(format!("case-team-preset-{}", case_id.get_value()));
    let note_id = StoredValue::new(format!("case-team-note-{}", case_id.get_value()));

    Effect::new(move |_| {
        if !editing.get() {
            return;
        }
        let query = search.get();
        candidates_loading.set(true);
        search_generation.update(|generation| *generation += 1);
        let generation = search_generation.get_untracked();
        spawn_local(async move {
            let response = crate::server_fns::case_team::search_assignable_users(query).await;
            if search_generation.get_untracked() != generation {
                return;
            }
            candidates.set(response.unwrap_or_default());
            candidates_loading.set(false);
        });
    });

    // People not already in the draft (removed rows count as in it: "Undo remove" restores them).
    let available = Memo::new(move |_| {
        let taken = draft.with(|rows| {
            rows.iter()
                .map(|row| row.user_id.clone())
                .collect::<Vec<_>>()
        });
        candidates
            .get()
            .into_iter()
            .filter(|user| !taken.contains(&user.id))
            .collect::<Vec<_>>()
    });

    let begin_edit = move |_| {
        let rows = team
            .get_untracked()
            .into_iter()
            .map(|member| {
                make_draft(
                    member.user_id.clone(),
                    member.full_name(),
                    member.role,
                    member.capabilities.clone(),
                )
            })
            .collect::<Vec<_>>();
        draft.set(rows);
        selected.set(Vec::new());
        search.set(String::new());
        note.set(String::new());
        feedback.set(None);
        editing.set(true);
    };

    let cancel_edit = move |_| {
        editing.set(false);
        selected.set(Vec::new());
        feedback.set(None);
    };

    let add_selected = move |_| {
        let picked = selected.get_untracked();
        if picked.is_empty() {
            return;
        }
        let caps = CasePreset::from_slug(&preset.get_untracked())
            .unwrap_or(CasePreset::Contributor)
            .capabilities();
        let people = candidates.get_untracked();
        draft.update(|rows| {
            for user_id in &picked {
                if rows.iter().any(|row| &row.user_id == user_id) {
                    continue;
                }
                if let Some(user) = people.iter().find(|user| &user.id == user_id) {
                    rows.push(make_draft(
                        user.id.clone(),
                        user.full_name(),
                        user.role,
                        caps.clone(),
                    ));
                }
            }
        });
        selected.set(Vec::new());
    };

    let save_edit = move |_| {
        let originals = team.get_untracked();
        let mut changes: Vec<(String, Option<Vec<CaseCapability>>)> = Vec::new();
        for row in draft.get_untracked() {
            let caps = row.caps.get_untracked();
            let removed = row.removed.get_untracked();
            match originals.iter().find(|member| member.user_id == row.user_id) {
                Some(member) => {
                    if removed || caps.is_empty() {
                        changes.push((row.user_id.clone(), None));
                    } else if !same_caps(&member.capabilities, &caps) {
                        changes.push((row.user_id.clone(), Some(caps)));
                    }
                }
                None => {
                    if !removed && !caps.is_empty() {
                        changes.push((row.user_id.clone(), Some(caps)));
                    }
                }
            }
        }
        if changes.is_empty() {
            editing.set(false);
            feedback.set(None);
            return;
        }
        saving.set(true);
        feedback.set(None);
        let note_text = note.get_untracked();
        spawn_local(async move {
            let outcome = crate::server_fns::case_team::save_case_team(
                case_id.get_value(),
                changes,
                note_text,
            )
            .await;
            match outcome {
                Ok(message) => {
                    feedback.set(Some(Ok(message)));
                    editing.set(false);
                    reload.update(|value| *value += 1);
                    state.refresh_badges();
                }
                Err(error) => feedback.set(Some(Err(err_text(error)))),
            }
            saving.set(false);
        });
    };

    // Whether saving needs approval for anyone in the draft.
    let needs_request = move || {
        !is_site_admin
            && draft.with(|rows| {
                let originals = team.get_untracked();
                rows.iter().any(|row| {
                    if row.user_id == actor_user_id() {
                        return false;
                    }
                    let caps = row.caps.get();
                    let removed = row.removed.get();
                    match originals.iter().find(|member| member.user_id == row.user_id) {
                        Some(member) => removed || !same_caps(&member.capabilities, &caps),
                        None => !removed && !caps.is_empty(),
                    }
                })
            })
    };

    let helper = if is_site_admin {
        "Everyone with access to this case. Changes here also show on each person's user page."
    } else {
        "Everyone with access to this case. Changes to other people are sent to a site admin for approval; your own access changes right away."
    };

    // --- views --------------------------------------------------------------
    let read_view = move || {
        if let Some(message) = load_error.get() {
            return view! {
                <p class="text-sm text-rose-300" role="alert">"Could not load the case team: " {message}</p>
            }
            .into_any();
        }
        let members = team.get();
        if members.is_empty() {
            let text = if loading.get() { "Loading…" } else { "No one is assigned to this case yet." };
            return view! { <p class="text-sm text-slate-500">{text}</p> }.into_any();
        }
        members
            .into_iter()
            .map(|member| {
                let href = format!("/admin/users/{}", member.user_id);
                let name = member.full_name();
                let role = member.role;
                let access = preset_label(&member.capabilities)
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        member
                            .capabilities
                            .iter()
                            .map(|capability| capability.label())
                            .collect::<Vec<_>>()
                            .join(", ")
                    });
                view! {
                    <div class="flex flex-wrap items-center justify-between gap-2 border-b border-slate-800 py-3 last:border-b-0">
                        <div class="flex min-w-0 flex-wrap items-center gap-2">
                            <A href=href attr:class="text-sm font-medium text-primary-300 hover:text-primary-200">
                                {name}
                            </A>
                            <span class=badge(role.badge_classes())>{role.label()}</span>
                        </div>
                        <span class="text-xs text-slate-400">{access}</span>
                    </div>
                }
            })
            .collect_view()
            .into_any()
    };

    let draft_rows = move || {
        view! {
            <Show
                when=move || !draft.get().is_empty()
                fallback=|| view! { <p class="text-sm text-slate-500">"No one is assigned to this case yet."</p> }
            >
                <For each=move || draft.get() key=|row| row.user_id.clone() let:row>
                    {
                        let row_caps = row.caps;
                        let row_removed = row.removed;
                        let checkboxes = CaseCapability::ALL
                            .into_iter()
                            .map(|capability| {
                                let unmet = move || {
                                    let held = row_caps.get();
                                    capability.requires().iter().any(|required| !held.contains(required))
                                };
                                view! {
                                    <label
                                        class="inline-flex items-center gap-1.5 text-xs"
                                        class=("text-slate-300", move || !unmet())
                                        class=("text-slate-600", unmet)
                                    >
                                        <input
                                            type="checkbox"
                                            class="h-3.5 w-3.5 rounded border-slate-600 bg-slate-950"
                                            prop:checked=move || row_caps.get().contains(&capability)
                                            prop:disabled=move || row_removed.get() || unmet()
                                            on:change=move |event| {
                                                let enabled = event_target_checked(&event);
                                                row_caps.update(|caps| {
                                                    if enabled {
                                                        if !caps.contains(&capability) {
                                                            caps.push(capability);
                                                        }
                                                    } else {
                                                        caps.retain(|held| *held != capability);
                                                        // Drop anything that depended on it.
                                                        loop {
                                                            let held = caps.clone();
                                                            let before = caps.len();
                                                            caps.retain(|c| c.requires().iter().all(|r| held.contains(r)));
                                                            if caps.len() == before {
                                                                break;
                                                            }
                                                        }
                                                    }
                                                });
                                            }
                                        />
                                        {capability.label()}
                                    </label>
                                }
                            })
                            .collect_view();
                        view! {
                            <div class=move || {
                                if row_removed.get() {
                                    "border-b border-slate-800 py-3 last:border-b-0 opacity-50"
                                } else {
                                    "border-b border-slate-800 py-3 last:border-b-0"
                                }
                            }>
                                <div class="flex flex-wrap items-center justify-between gap-2">
                                    <div class="flex flex-wrap items-center gap-2">
                                        <span class="text-sm font-medium text-slate-200">{row.name.clone()}</span>
                                        <span class=badge(row.role.badge_classes())>{row.role.label()}</span>
                                    </div>
                                    <button
                                        type="button"
                                        on:click=move |_| row_removed.update(|removed| *removed = !*removed)
                                        class="rounded-full border border-rose-500/40 px-2 py-1 text-xs font-medium text-rose-300 hover:bg-rose-500/10"
                                    >
                                        {move || if row_removed.get() { "Undo remove" } else { "Remove" }}
                                    </button>
                                </div>
                                <div class="mt-2 flex flex-wrap gap-x-4 gap-y-1.5">{checkboxes}</div>
                            </div>
                        }
                    }
                </For>
            </Show>
        }
    };

    let picker = move || {
        view! {
            <div class="mt-5 rounded-lg border border-slate-800 bg-slate-950/40 p-3">
                <h4 class="text-sm font-semibold text-slate-200">"Add people"</h4>
                <div class="mt-3 grid gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(10rem,auto)] sm:items-end">
                    <div>
                        <label class="block text-xs font-medium text-slate-400" for=search_id.get_value()>
                            "Search volunteers"
                        </label>
                        <input
                            id=search_id.get_value()
                            type="search"
                            class="mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500"
                            placeholder="Name or email"
                            prop:value=move || search.get()
                            on:input=move |event| search.set(event_target_value(&event))
                        />
                    </div>
                    <div>
                        <label class="block text-xs font-medium text-slate-400" for=preset_id.get_value()>
                            "Give them"
                        </label>
                        <select
                            id=preset_id.get_value()
                            class="mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100"
                            prop:value=move || preset.get()
                            on:change=move |event| preset.set(event_target_value(&event))
                        >
                            {CasePreset::ALL
                                .into_iter()
                                .map(|p| view! { <option value=p.slug()>{p.label()}</option> })
                                .collect_view()}
                        </select>
                    </div>
                </div>
                <div
                    class="mt-3 max-h-64 overflow-y-auto rounded-xl border border-slate-800"
                    role="group"
                    aria-label="Volunteers to add"
                >
                    {move || {
                        let people = available.get();
                        if people.is_empty() {
                            let text = if candidates_loading.get() {
                                "Loading…"
                            } else if search.get().trim().is_empty() {
                                "Everyone available is already on this case."
                            } else {
                                "No matching volunteers."
                            };
                            return view! { <p class="px-3 py-2 text-xs text-slate-500">{text}</p> }.into_any();
                        }
                        people
                            .into_iter()
                            .map(|user| {
                                let id = StoredValue::new(user.id.clone());
                                view! {
                                    <label class="flex cursor-pointer items-center gap-2 border-b border-slate-800 px-3 py-2 text-sm text-slate-200 last:border-b-0 hover:bg-slate-800/60">
                                        <input
                                            type="checkbox"
                                            class="h-4 w-4 rounded border-slate-600 bg-slate-950"
                                            prop:checked=move || selected.with(|ids| ids.contains(&id.get_value()))
                                            on:change=move |event| {
                                                let checked = event_target_checked(&event);
                                                let id = id.get_value();
                                                selected.update(|ids| {
                                                    if checked {
                                                        if !ids.contains(&id) {
                                                            ids.push(id);
                                                        }
                                                    } else {
                                                        ids.retain(|existing| *existing != id);
                                                    }
                                                });
                                            }
                                        />
                                        <span class="min-w-0 flex-1 truncate">{user.full_name()}</span>
                                        <span class=badge(user.role.badge_classes())>{user.role.label()}</span>
                                    </label>
                                }
                            })
                            .collect_view()
                            .into_any()
                    }}
                </div>
                <div class="mt-3 flex flex-wrap items-center gap-3">
                    <button
                        type="button"
                        on:click=add_selected
                        prop:disabled=move || selected.with(Vec::is_empty)
                        class="rounded-full bg-primary-500 px-3 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                    >
                        {move || match selected.with(Vec::len) {
                            0 => "Add selected".to_string(),
                            1 => "Add 1 person".to_string(),
                            n => format!("Add {n} people"),
                        }}
                    </button>
                    <button
                        type="button"
                        on:click=move |_| {
                            let ids = available.get_untracked().into_iter().map(|user| user.id).collect();
                            selected.set(ids);
                        }
                        class="text-xs font-medium text-slate-400 hover:text-slate-200"
                    >
                        "Select all shown"
                    </button>
                    <span class="text-xs text-slate-500">"Added people appear above. Nothing is saved until you click Save."</span>
                </div>
            </div>
        }
    };

    let note_field = move || {
        needs_request().then(|| {
            view! {
                <div class="mt-4">
                    <label class="block text-xs font-medium text-slate-400" for=note_id.get_value()>
                        "Request note"
                    </label>
                    <textarea
                        id=note_id.get_value()
                        rows="2"
                        maxlength="1000"
                        class="mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500"
                        placeholder="Optional context for the site admin"
                        prop:value=move || note.get()
                        on:input=move |event| note.set(event_target_value(&event))
                    ></textarea>
                </div>
            }
        })
    };

    let header_buttons = move || {
        if editing.get() {
            view! {
                <button
                    type="button"
                    on:click=save_edit
                    prop:disabled=move || saving.get()
                    class="rounded-full bg-primary-500 px-3 py-1.5 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                >
                    {move || match (saving.get(), needs_request()) {
                        (true, _) => "Saving…",
                        (false, true) => "Save and request approval",
                        (false, false) => "Save",
                    }}
                </button>
                <button
                    type="button"
                    on:click=cancel_edit
                    class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-300 hover:bg-slate-800"
                >
                    "Cancel"
                </button>
            }
            .into_any()
        } else {
            view! {
                <button
                    type="button"
                    on:click=begin_edit
                    prop:disabled=move || loading.get() || load_error.get().is_some()
                    class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800 disabled:opacity-50"
                >
                    "Edit team"
                </button>
            }
            .into_any()
        }
    };

    view! {
        <section id="case-team" class="rounded-card border border-slate-800 bg-slate-900 shadow-card p-4">
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div class="min-w-0">
                    <h3 class="text-base font-semibold text-slate-100">"Case team"</h3>
                    <p class="mt-1 text-sm text-slate-400">{helper}</p>
                </div>
                <div class="flex shrink-0 gap-2">{header_buttons}</div>
            </div>
            <div class="mt-3">
                {move || if editing.get() {
                    view! { {draft_rows} {picker} {note_field} }.into_any()
                } else {
                    read_view().into_any()
                }}
            </div>
            {move || feedback.get().map(|result| match result {
                Ok(message) => view! {
                    <p class="mt-3 text-sm text-emerald-300" role="status">{message}</p>
                }.into_any(),
                Err(message) => view! {
                    <p class="mt-3 text-sm text-rose-300" role="alert">{message}</p>
                }.into_any(),
            })}
        </section>
    }
}
