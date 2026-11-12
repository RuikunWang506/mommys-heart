use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::components::guard::require_login;
use crate::components::layout::Layout;
use crate::components::loading::Loading;
use crate::components::profile_link::ProfileLink;
use crate::server_fns::capabilities::CaseCapability;
use crate::server_fns::cases::{load_case_summaries_for_user, CaseSummary};
use crate::server_fns::channels::{
    archive_channel, create_channel, list_channels, normalize_channel_name, Channel,
};
use crate::server_fns::err_text;
use crate::server_fns::message::{Message, MAX_MESSAGE_BODY_CHARS};
use crate::state::AppState;

/// Case Chat: one chat thread per case. Reading requires `ViewCase`; posting
/// requires `SendMessages` and an active channel.
#[component]
pub fn InboxPage() -> impl IntoView {
    let state = expect_context::<AppState>();

    let cases = RwSignal::new(Vec::<CaseSummary>::new());
    let total = RwSignal::new(0i64);
    let load_error = RwSignal::new(None::<String>);

    let selected = RwSignal::new(None::<String>);
    // Mobile only: whether the user has opened a chat (collapses the list and
    // shows the thread full-screen). Desktop always shows both panes.
    let viewing = RwSignal::new(false);
    let search = RwSignal::new(String::new());
    let debounced_search = RwSignal::new(String::new());
    // How many rows the current window requests; grows on "Load more".
    const PAGE: i64 = 15;
    let window = RwSignal::new(PAGE);
    let loading = RwSignal::new(false);

    // Channels of whichever case is selected. They render as sub-items of that
    // case in the list on the left, so the state lives here rather than in the
    // chat pane.
    let channels = RwSignal::new(Vec::<Channel>::new());
    let active = RwSignal::new(None::<String>);
    let loading_channels = RwSignal::new(false);
    let channel_error = RwSignal::new(String::new());
    // Bumped after saving channel edits to refetch the list.
    let reload = RwSignal::new(0u32);
    // Edit mode: staged additions/archives that only hit the server on "Save".
    let editing = RwSignal::new(false);
    let pending_adds = RwSignal::new(Vec::<String>::new());
    let pending_archives = RwSignal::new(Vec::<String>::new());
    let draft = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    let input_class = "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40";

    // (Re)load the window whenever the debounced search or window size changes
    Effect::new(move |_| {
        let count = window.get();
        let q = debounced_search.get();
        if !state.is_authenticated() {
            return;
        }
        loading.set(true);
        spawn_local(async move {
            match load_case_summaries_for_user(0, count, q).await {
                Ok(page) => {
                    cases.set(page.items);
                    total.set(page.total);
                    load_error.set(None);
                }
                Err(e) => load_error.set(Some(err_text(e))),
            }
            loading.set(false);
        });
    });

    // Load the selected case's channels. Also runs when `reload` is bumped after
    // saving channel edits. Switching cases drops any in-progress edit.
    Effect::new(move |prev: Option<Option<String>>| {
        reload.track();
        let sel = selected.get();
        let case_changed = prev.as_ref() != Some(&sel);
        if case_changed {
            editing.set(false);
            pending_adds.set(Vec::new());
            pending_archives.set(Vec::new());
            draft.set(String::new());
            channel_error.set(String::new());
            // Drop the previous case's channels so its list never shows up
            // under a different case while the new one loads.
            channels.set(Vec::new());
            active.set(None);
        }
        let Some(case_id) = sel.clone() else {
            channels.set(Vec::new());
            active.set(None);
            loading_channels.set(false);
            return sel;
        };
        loading_channels.set(true);
        spawn_local(async move {
            match list_channels(case_id).await {
                Ok(list) => {
                    // Keep the open channel if it still exists, else fall back
                    // to the first one the server sent.
                    let keep = active
                        .get_untracked()
                        .filter(|id| list.iter().any(|c| &c.id == id))
                        .or_else(|| list.first().map(|c| c.id.clone()));
                    active.set(keep);
                    channels.set(list);
                }
                Err(e) => {
                    channels.set(Vec::new());
                    active.set(None);
                    channel_error.set(err_text(e));
                }
            }
            loading_channels.set(false);
        });
        sel
    });

    require_login(state, move || {
        // Auto-select the first accessible case so the chat is populated on load.
        if selected.get_untracked().is_none() {
            if let Some(first) = cases.get().first() {
                selected.set(Some(first.id.clone()));
            }
        }

        // Display title for a case: the case name with its owner appended.
        let title_for = move |c: &CaseSummary| format!("{} · {}", c.name, c.owner_full_name());

        let case_list = move || {
            if let Some(msg) = load_error.get() {
                return view! {
                    <p class="text-sm text-rose-300">"Could not load case chats: " {msg}</p>
                }
                .into_any();
            }
            let all = cases.get();
            if all.is_empty() {
                let msg = if loading.get() {
                    "Loading\u{2026}"
                } else if search.get().trim().is_empty() {
                    "You have no case chats yet."
                } else {
                    "No cases match your search."
                };
                return view! { <p class="text-sm text-slate-400">{msg}</p> }.into_any();
            }
            all
                .into_iter()
                .map(|c| {
                    let case_id = c.id.clone();
                    // `StoredValue` keeps the id `Copy`-accessible so it can be
                    // read from more than one reactive closure.
                    let stored_id = StoredValue::new(case_id.clone());
                    let is_selected =
                        move || stored_id.with_value(|id| selected.get().as_deref() == Some(id.as_str()));
                    // Total unread across this case's channels, for the list badge.
                    let case_unread = move || {
                        stored_id.with_value(|id| {
                            state
                                .unread
                                .get()
                                .iter()
                                .filter(|u| &u.case_id == id)
                                .map(|u| u.count)
                                .sum::<i64>()
                        })
                    };
                    let count = c.message_count;
                    let name = title_for(&c);
                    let can_manage = c.capabilities.contains(&CaseCapability::ManageChannels);
                    let select = {
                        let case_id = case_id.clone();
                        move |_| {
                            // Selecting a case only expands its channels; the
                            // thread opens once a channel is picked (matters on
                            // mobile, where the two panes are exclusive).
                            if selected.get_untracked().as_deref() != Some(case_id.as_str()) {
                                selected.set(Some(case_id.clone()));
                            }
                        }
                    };
                    let nav_case_id = case_id.clone();
                    view! {
                        <div class="space-y-1">
                        <button
                            on:click=select
                            class=move || {
                                let base = "w-full rounded-xl border p-3 text-left transition-colors";
                                if is_selected() {
                                    format!("{base} border-primary-500/50 bg-slate-800")
                                } else {
                                    format!("{base} border-slate-800 bg-slate-900 hover:bg-slate-800")
                                }
                            }
                        >
                            <div class="flex items-center justify-between gap-2">
                                <span class="min-w-0 truncate text-sm font-medium text-slate-200">{name}</span>
                                <span class="flex shrink-0 items-center gap-1.5 text-xs text-slate-500">
                                    {move || {
                                        let n = case_unread();
                                        (n > 0)
                                            .then(|| {
                                                view! {
                                                    <span class="inline-flex min-w-[1.1rem] items-center justify-center rounded-full bg-primary-500 px-1.5 text-[0.65rem] font-semibold leading-none text-white">
                                                        {n}
                                                    </span>
                                                }
                                            })
                                    }}
                                    <span>{count} " msgs"</span>
                                </span>
                            </div>
                        </button>
                        <Show when=is_selected>
                            <ChannelNav
                                case_id=nav_case_id.clone()
                                can_manage=can_manage
                                channels=channels
                                active=active
                                viewing=viewing
                                loading=loading_channels
                                error=channel_error
                                editing=editing
                                pending_adds=pending_adds
                                pending_archives=pending_archives
                                draft=draft
                                busy=busy
                                reload=reload
                            />
                        </Show>
                        </div>
                    }
                    .into_any()
                })
                .collect_view()
                .into_any()
        };

        let footer = move || {
            let shown = cases.get().len() as i64;
            let tot = total.get();
            if tot == 0 {
                return ().into_any();
            }
            let more = shown < tot;
            view! {
                <div class="mt-2 flex items-center justify-between">
                    <p class="text-xs text-slate-500">"Showing " {shown} " of " {tot}</p>
                    <Show when=move || more>
                        <button
                            on:click=move |_| window.update(|w| *w += PAGE)
                            prop:disabled=move || loading.get()
                            class="rounded-full border border-slate-700 px-3 py-1.5 text-xs font-medium text-slate-300 hover:bg-slate-800 disabled:opacity-50"
                        >
                            {move || if loading.get() { "Loading\u{2026}" } else { "Load more" }}
                        </button>
                    </Show>
                </div>
            }
            .into_any()
        };

        let thread = move || {
            match selected.get() {
                None => view! {
                    <div class="rounded-xl border border-dashed border-slate-700 p-8 text-center text-sm text-slate-500">
                        "Select a case to open its chat."
                    </div>
                }
                .into_any(),
                Some(id) => match cases.get().into_iter().find(|c| c.id == id) {
                    Some(c) => {
                        let title = title_for(&c);
                        // A declined case takes no new messages.
                        let can_send = c.capabilities.contains(&CaseCapability::SendMessages)
                            && c.status.accepts_changes();
                        view! {
                            <CaseChat
                                case_name=title
                                can_send=can_send
                                channels=channels
                                active=active
                                loading_channels=loading_channels
                            />
                        }
                        .into_any()
                    }
                    None => {
                        view! { <p class="text-sm text-slate-400">"Case not found."</p> }.into_any()
                    }
                },
            }
        };

        // Debounce the search
        let mut on_search = debounce(std::time::Duration::from_secs(1), move |val: String| {
            window.set(PAGE);
            debounced_search.set(val);
        });

        view! {
            <Layout title="Case Chat".to_string()>
                <div class="grid gap-6 lg:grid-cols-[22rem_1fr]">
                    <div class="space-y-2 lg:block" class:hidden=move || viewing.get()>
                        <input
                            class=input_class
                            placeholder="Search cases…"
                            prop:value=move || search.get()
                            on:input=move |ev| {
                                let val = event_target_value(&ev);
                                search.set(val.clone());
                                on_search(val);
                            }
                        />
                        {case_list}
                        {footer}
                    </div>
                    <div class="lg:block" class:hidden=move || !viewing.get()>
                        <Show when=move || viewing.get()>
                            <button
                                on:click=move |_| viewing.set(false)
                                class="mb-4 inline-flex items-center gap-1.5 rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800 lg:hidden"
                            >
                                "\u{2190} Back to chats"
                            </button>
                        </Show>
                        {thread}
                    </div>
                </div>
            </Layout>
        }
        .into_any()
    })
}

/// The [`Channel`] sub-navigation shown under the selected case in the list on
/// the left. Picking a channel opens its thread in the chat pane.
///
/// Every case has a permanent "Volunteer only" channel plus one or more shared
/// channels (starting with "General"). The server decides what lands here — a
/// client's `list_channels` response simply never contains the volunteer-only
/// channel — so this component renders whatever it is given rather than
/// filtering client-side.
///
/// `can_manage` (the `ManageChannels` capability, held only by "Full access")
/// is what reveals the "Edit" control. Editing stages additions and removals
/// locally: nothing is sent until "Save", and "Cancel" throws the draft away.
/// Every call is re-checked server-side; the flag only shapes the UI.
#[component]
fn ChannelNav(
    case_id: String,
    can_manage: bool,
    channels: RwSignal<Vec<Channel>>,
    active: RwSignal<Option<String>>,
    /// Mobile only: set when a channel is picked so the thread takes over the
    /// screen. Desktop shows both panes regardless.
    viewing: RwSignal<bool>,
    loading: RwSignal<bool>,
    error: RwSignal<String>,
    editing: RwSignal<bool>,
    pending_adds: RwSignal<Vec<String>>,
    pending_archives: RwSignal<Vec<String>>,
    draft: RwSignal<String>,
    busy: RwSignal<bool>,
    reload: RwSignal<u32>,
) -> impl IntoView {
    // `StoredValue` keeps the case id `Copy`-accessible so the handlers below
    // stay `FnMut` inside reactive closures.
    let case_id = StoredValue::new(case_id);
    let state = expect_context::<AppState>();

    let is_staged_archive = move |id: &str| pending_archives.get().iter().any(|d| d == id);

    let toggle_archive = move |id: String| {
        pending_archives.update(|list| {
            if let Some(pos) = list.iter().position(|d| d == &id) {
                list.remove(pos);
            } else {
                list.push(id);
            }
        });
    };

    // Stage the typed name after the same validation the server applies, so
    // obvious mistakes are caught before "Save". An empty draft is a no-op, so
    // "Save" can call this too and never silently drop a half-typed name.
    let try_stage_draft = move || -> Result<(), String> {
        let raw = draft.get_untracked();
        if raw.trim().is_empty() {
            return Ok(());
        }
        let name = normalize_channel_name(&raw)?;
        let taken = channels
            .get_untracked()
            .iter()
            .any(|c| c.name.eq_ignore_ascii_case(&name))
            || pending_adds
                .get_untracked()
                .iter()
                .any(|n| n.eq_ignore_ascii_case(&name));
        if taken {
            return Err(format!("This case already has a \"{name}\" channel."));
        }
        pending_adds.update(|list| list.push(name));
        draft.set(String::new());
        Ok(())
    };

    let commit_draft = move || match try_stage_draft() {
        Ok(()) => error.set(String::new()),
        Err(msg) => error.set(msg),
    };

    let cancel = move |_| {
        editing.set(false);
        pending_adds.set(Vec::new());
        pending_archives.set(Vec::new());
        draft.set(String::new());
        error.set(String::new());
    };

    // Apply the staged edits. Anything that fails stays staged so the user can
    // see what went wrong and retry without retyping it.
    let save = move |_| {
        if let Err(msg) = try_stage_draft() {
            error.set(msg);
            return;
        }
        let case_id = case_id.get_value();
        let adds = pending_adds.get_untracked();
        let archives = pending_archives.get_untracked();
        if adds.is_empty() && archives.is_empty() {
            editing.set(false);
            error.set(String::new());
            return;
        }
        busy.set(true);
        spawn_local(async move {
            let mut failed_adds = Vec::new();
            let mut failed_archives = Vec::new();
            let mut first_error = None::<String>;

            for name in adds {
                if let Err(e) = create_channel(case_id.clone(), name.clone()).await {
                    first_error.get_or_insert_with(|| err_text(e));
                    failed_adds.push(name);
                }
            }
            for id in archives {
                if let Err(e) = archive_channel(id.clone()).await {
                    first_error.get_or_insert_with(|| err_text(e));
                    failed_archives.push(id);
                }
            }

            pending_adds.set(failed_adds);
            pending_archives.set(failed_archives);
            match first_error {
                Some(msg) => error.set(msg),
                None => {
                    error.set(String::new());
                    editing.set(false);
                }
            }
            busy.set(false);
            reload.update(|r| *r += 1);
        });
    };

    let rows = move || {
        let list = channels.get();
        if list.is_empty() {
            let text = if loading.get() {
                "Loading channels\u{2026}"
            } else {
                "This case has no channels."
            };
            return view! { <p class="px-2 py-1 text-xs text-slate-500">{text}</p> }.into_any();
        }
        list.into_iter()
            .map(|c| {
                let id = StoredValue::new(c.id.clone());
                let is_active =
                    move || id.with_value(|id| active.get().as_deref() == Some(id.as_str()));
                let staged = move || id.with_value(|id| is_staged_archive(id));
                // Unread messages for this channel, from the app-wide badge state.
                let unread = move || {
                    id.with_value(|id| {
                        state
                            .unread
                            .get()
                            .iter()
                            .find(|u| &u.channel_id == id)
                            .map(|u| u.count)
                            .unwrap_or(0)
                    })
                };
                let restricted = c.kind.is_restricted();
                let select = move |_| {
                    active.set(Some(id.get_value()));
                    viewing.set(true);
                };
                // The volunteer-only channel is permanent, so it has no archive control.
                let archive = if c.is_archivable() {
                    view! {
                        <Show when=move || editing.get()>
                            <button
                                on:click=move |_| toggle_archive(id.get_value())
                                prop:disabled=move || busy.get()
                                title=move || {
                                    if staged() { "Keep channel active" } else { "Archive channel" }
                                }
                                class="shrink-0 rounded px-1.5 text-xs font-semibold text-slate-500 hover:text-rose-400 disabled:opacity-50"
                            >
                                {move || if staged() { "\u{21ba}" } else { "\u{00d7}" }}
                            </button>
                        </Show>
                    }
                    .into_any()
                } else {
                    ().into_any()
                };
                let count = c.message_count;
                let name = c.name.clone();
                view! {
                    <div class="flex items-center gap-0.5">
                        <button
                            on:click=select
                            class=move || {
                                let base = "flex min-w-0 flex-1 items-center gap-2 rounded-lg px-2 py-1.5 text-left text-sm transition-colors";
                                if staged() {
                                    format!("{base} text-slate-500 line-through")
                                } else if is_active() {
                                    format!("{base} bg-primary-500/15 font-semibold text-primary-300")
                                } else if unread() > 0 {
                                    format!("{base} font-semibold text-primary-300 hover:bg-slate-800")
                                } else {
                                    format!("{base} text-slate-300 hover:bg-slate-800")
                                }
                            }
                        >
                            <span class="shrink-0 text-xs text-slate-500">
                                {if restricted { "\u{1f512}" } else { "#" }}
                            </span>
                            <span class="min-w-0 truncate">{name}</span>
                            {c.archived.then(|| view! {
                                <span class="rounded bg-slate-700 px-1 text-[0.6rem] uppercase text-slate-300">
                                    "Archived"
                                </span>
                            })}
                            {move || {
                                let n = unread();
                                if n > 0 {
                                    view! {
                                        <span class="ml-auto inline-flex min-w-[1.1rem] shrink-0 items-center justify-center rounded-full bg-primary-500 px-1.5 text-[0.65rem] font-semibold leading-none text-white">
                                            {n}
                                        </span>
                                    }
                                    .into_any()
                                } else {
                                    view! {
                                        <span class="ml-auto shrink-0 text-xs text-slate-500">
                                            {count}
                                        </span>
                                    }
                                    .into_any()
                                }
                            }}
                        </button>
                        {archive}
                    </div>
                }
                .into_any()
            })
            .collect_view()
            .into_any()
    };

    // Channels queued for creation, shown alongside the real ones until saved.
    let staged_rows = move || {
        if !editing.get() {
            return ().into_any();
        }
        pending_adds
            .get()
            .into_iter()
            .enumerate()
            .map(|(i, name)| {
                view! {
                    <div class="flex items-center gap-0.5">
                        <span class="flex min-w-0 flex-1 items-center gap-2 rounded-lg px-2 py-1.5 text-sm text-slate-400">
                            <span class="shrink-0 text-xs text-slate-500">"#"</span>
                            <span class="min-w-0 truncate">{name}</span>
                            <span class="ml-auto shrink-0 text-xs text-slate-500">"new"</span>
                        </span>
                        <button
                            on:click=move |_| pending_adds.update(|list| { list.remove(i); })
                            prop:disabled=move || busy.get()
                            title="Discard new channel"
                            class="shrink-0 rounded px-1.5 text-xs font-semibold text-slate-500 hover:text-rose-400 disabled:opacity-50"
                        >
                            "\u{00d7}"
                        </button>
                    </div>
                }
                .into_any()
            })
            .collect_view()
            .into_any()
    };

    let controls = move || {
        if !can_manage {
            return ().into_any();
        }
        if !editing.get() {
            return view! {
                <button
                    on:click=move |_| {
                        editing.set(true);
                        error.set(String::new());
                    }
                    class="mt-1 rounded-full px-2 py-1 text-xs font-medium text-slate-500 hover:bg-slate-800 hover:text-slate-300"
                >
                    "Edit channels"
                </button>
            }
            .into_any();
        }
        view! {
            <div class="mt-1 space-y-2">
                <div class="flex gap-1">
                    <input
                        class="min-w-0 flex-1 rounded-xl border border-slate-700 bg-slate-900 px-2 py-1 text-xs text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none"
                        placeholder="New channel name"
                        prop:value=move || draft.get()
                        prop:disabled=move || busy.get()
                        on:input=move |ev| draft.set(event_target_value(&ev))
                        on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                            if ev.key() == "Enter" {
                                ev.prevent_default();
                                commit_draft();
                            }
                        }
                    />
                    <button
                        on:click=move |_| commit_draft()
                        prop:disabled=move || busy.get() || draft.get().trim().is_empty()
                        class="shrink-0 rounded-full border border-dashed border-slate-600 px-2 py-1 text-xs font-medium text-slate-300 hover:bg-slate-800 disabled:opacity-50"
                    >
                        "+ Add"
                    </button>
                </div>
                <div class="flex gap-1">
                    <button
                        on:click=save
                        prop:disabled=move || busy.get()
                        class="rounded-full bg-primary-500 px-2.5 py-1 text-xs font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                    >
                        {move || if busy.get() { "Saving\u{2026}" } else { "Save" }}
                    </button>
                    <button
                        on:click=cancel
                        prop:disabled=move || busy.get()
                        class="rounded-full border border-slate-700 px-2.5 py-1 text-xs font-medium text-slate-300 hover:bg-slate-800 disabled:opacity-50"
                    >
                        "Cancel"
                    </button>
                </div>
            </div>
        }
        .into_any()
    };

    view! {
        <div class="ml-3 border-l border-slate-800 pl-2">
            <p class="px-2 pb-0.5 pt-1 text-[0.7rem] font-semibold uppercase tracking-wide text-slate-500">
                "Channels"
            </p>
            {rows}
            {staged_rows}
            {controls}
            <Show when=move || !error.get().is_empty()>
                <p class="px-2 py-1 text-xs text-rose-300">{move || error.get()}</p>
            </Show>
        </div>
    }
    .into_any()
}

/// A single case's chat: the message thread for whichever channel is open in
/// the case list on the left.
///
/// `can_send` gates posting and is re-checked server-side on every call; the
/// flag only shapes the UI.
#[component]
fn CaseChat(
    case_name: String,
    can_send: bool,
    channels: RwSignal<Vec<Channel>>,
    active: RwSignal<Option<String>>,
    loading_channels: RwSignal<bool>,
) -> impl IntoView {
    let input_class = "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40";

    let active_channel = move || {
        let id = active.get()?;
        channels.get().into_iter().find(|c| c.id == id)
    };

    let subtitle = move || match active_channel() {
        Some(c) => {
            let prefix = if c.kind.is_restricted() {
                "\u{1f512} "
            } else {
                "# "
            };
            format!("{prefix}{}", c.name)
        }
        None => "No channel open".to_string(),
    };

    // Remounts whenever the open channel changes, so each channel gets a fresh
    // thread (its own pagination window and scroll position) instead of one
    // shared message list.
    let thread = move || match active_channel() {
        Some(c) => {
            let restricted = c.kind.is_restricted();
            let is_active = c.is_active();
            view! {
                <ChannelThread
                    channel_id=c.id
                    channel_name=c.name
                    restricted=restricted
                    archived=c.archived
                    can_send=can_send && is_active
                    input_class=input_class
                />
            }
            .into_any()
        }
        None if loading_channels.get() => view! {
            <div class="flex-1 p-4">
                <Loading label="Loading channels\u{2026}" />
            </div>
        }
        .into_any(),
        None => view! {
            <div class="flex-1 p-8 text-center text-sm text-slate-500">
                "This case has no channels to show."
            </div>
        }
        .into_any(),
    };

    view! {
        <div class="flex h-[70dvh] flex-col rounded-card border border-slate-800 bg-slate-900 shadow-card lg:h-[calc(100dvh-12rem)]">
            <div class="border-b border-slate-800 p-4">
                <h2 class="truncate text-lg font-semibold">{case_name}</h2>
                <p class="mt-0.5 truncate text-sm text-slate-500">{subtitle}</p>
            </div>
            {thread}
        </div>
    }
    .into_any()
}

/// The message thread for one channel.
///
/// Messages are paginated newest-first: the thread opens scrolled to the latest
/// message and a "Load earlier messages" button at the top pages older messages
/// in on demand, so a long conversation never loads all at once.
#[component]
fn ChannelThread(
    channel_id: String,
    channel_name: String,
    /// Whether this is the volunteer-only channel, which gets a visible banner
    /// so staff always know clients cannot read what they post here.
    restricted: bool,
    archived: bool,
    can_send: bool,
    input_class: &'static str,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    let current_user = state.current_user_summary.get_untracked();
    let me = current_user
        .as_ref()
        .map(|user| user.id.clone())
        .unwrap_or_default();
    let can_export = current_user
        .as_ref()
        .is_some_and(|user| user.role.has_operations_admin_permissions());

    // This channel's messages live here — loaded on demand for the open channel.
    let messages = RwSignal::new(Vec::<Message>::new());
    // Tracks the in-flight fetch of the newest messages so the thread can show a
    // loading indicator instead of a premature "no messages" state.
    let loading = RwSignal::new(true);

    // How many of the most recent messages to request; grows on "Load more".
    const MSG_PAGE: i64 = 20;
    let limit = RwSignal::new(MSG_PAGE);
    let total = RwSignal::new(0i64);

    // Scroll container; used to jump to the latest message on load and after
    // sending. Growing `limit` (loading earlier messages) deliberately does not
    // scroll, so the user stays where they were reading.
    let scroll_ref = NodeRef::<leptos::html::Div>::new();
    let scroll_to_bottom = move || {
        if let Some(el) = scroll_ref.get_untracked() {
            request_animation_frame(move || el.set_scroll_top(el.scroll_height()));
        }
    };
    let did_initial_scroll = RwSignal::new(false);

    // (Re)load the newest `limit` messages whenever the window grows. Browser
    // only; the SSR branch errors and is ignored.
    {
        let channel_id = channel_id.clone();
        Effect::new(move |_| {
            let lim = limit.get();
            let channel_id = channel_id.clone();
            loading.set(true);
            spawn_local(async move {
                if let Ok(page) =
                    crate::server_fns::cases::list_messages_page(channel_id, lim).await
                {
                    messages.set(page.items);
                    total.set(page.total);
                    state.refresh_unread();
                }
                loading.set(false);
            });
        });
    }

    // Once messages first arrive, jump to the latest one.
    Effect::new(move |_| {
        let has = !messages.get().is_empty();
        if has && !did_initial_scroll.get_untracked() {
            did_initial_scroll.set(true);
            scroll_to_bottom();
        }
    });

    let body = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let sending = RwSignal::new(false);

    let send = {
        let channel_id = channel_id.clone();
        move |_| {
            if sending.get_untracked() {
                return;
            }
            let body_val = body.get_untracked();
            if body_val.trim().is_empty() {
                error.set("Enter a message first.".to_string());
                return;
            }
            let channel_id = channel_id.clone();
            sending.set(true);
            spawn_local(async move {
                match crate::server_fns::cases::send_message(channel_id, body_val).await {
                    Ok(msg) => {
                        messages.update(|all| all.push(msg));
                        body.set(String::new());
                        error.set(String::new());
                        total.update(|t| *t += 1);
                        scroll_to_bottom();
                    }
                    Err(e) => error.set(err_text(e)),
                }
                sending.set(false);
            });
        }
    };

    let messages_view = {
        let me = me.clone();
        move || {
            let msgs = messages.get();
            let shown = msgs.len() as i64;

            // "Load more" pages in earlier messages at the top of the thread.
            let load_more = if shown < total.get() {
                view! {
                    <button
                        on:click=move |_| limit.update(|l| *l += MSG_PAGE)
                        class="mx-auto block rounded-full border border-slate-700 px-3 py-1.5 text-xs font-medium text-slate-300 hover:bg-slate-800"
                    >
                        "Load earlier messages (" {shown} " of " {move || total.get()} ")"
                    </button>
                }
                .into_any()
            } else {
                ().into_any()
            };

            if msgs.is_empty() {
                if loading.get() {
                    return view! { <Loading label="Loading messages\u{2026}" /> }.into_any();
                }
                return view! {
                    <p class="text-sm text-slate-500">"No messages yet. Start the conversation."</p>
                }
                .into_any();
            }

            let rows = msgs
                .into_iter()
                .map(|m| {
                    let mine = m.author_id == me;
                    let row = if mine {
                        "flex justify-end"
                    } else {
                        "flex justify-start"
                    };
                    let bubble = if mine {
                        "max-w-[80%] rounded-2xl bg-primary-500/20 px-4 py-2 text-sm text-slate-100"
                    } else {
                        "max-w-[80%] rounded-2xl border border-slate-800 bg-slate-900 px-4 py-2 text-sm text-slate-100"
                    };
                    view! {
                        <div class=row>
                            <div class=bubble>
                                <p class="text-xs font-medium text-slate-400">
                                    <ProfileLink
                                        user_id=m.author_id.clone()
                                        name=m.author.clone()
                                        class="text-xs"
                                    />
                                    " \u{00b7} "
                                    {m.sent_at}
                                </p>
                                <p class="mt-0.5 whitespace-pre-wrap">{m.body}</p>
                            </div>
                        </div>
                    }
                    .into_any()
                })
                .collect_view();

            view! {
                {load_more}
                {rows}
            }
            .into_any()
        }
    };

    let banner = if archived {
        view! {
            <p class="border-b border-slate-700 bg-slate-800 px-4 py-2 text-xs font-medium text-slate-300">
                "Archived channel — its preserved history is read-only."
            </p>
        }
        .into_any()
    } else if restricted {
        view! {
            <p class="border-b border-amber-500/20 bg-amber-500/10 px-4 py-2 text-xs font-medium text-amber-300">
                "\u{1f512} Private channel \u{2014} only volunteers and admins on this case can see these messages."
            </p>
        }
        .into_any()
    } else {
        ().into_any()
    };

    let placeholder = format!("Message {channel_name}");
    let export_href = format!("/api/channels/{channel_id}/transcript.csv");

    view! {
        <div class="flex min-h-0 flex-1 flex-col">
            {banner}
            {can_export.then(|| view! {
                <div class="border-b border-slate-800 px-4 py-2 text-right">
                    <a
                        href=export_href
                        class="text-xs font-medium text-primary-300 hover:text-primary-200"
                    >
                        "Download audited CSV transcript"
                    </a>
                </div>
            })}
            <div node_ref=scroll_ref class="flex-1 space-y-3 overflow-y-auto p-4">
                {messages_view}
            </div>
            <div class="relative z-10 shrink-0 border-t border-slate-800 bg-slate-900 p-4">
                {if can_send {
                    view! {
                        <div class="space-y-2">
                            <Show when=move || !error.get().is_empty()>
                                <p class="text-xs text-rose-300">{move || error.get()}</p>
                            </Show>
                            <div class="flex gap-2">
                                <input
                                    class=input_class
                                    placeholder=placeholder
                                    maxlength=MAX_MESSAGE_BODY_CHARS
                                    prop:value=move || body.get()
                                    prop:disabled=move || sending.get()
                                    on:input=move |ev| body.set(event_target_value(&ev))
                                />
                                <button
                                    on:click=send
                                    prop:disabled=move || sending.get() || body.get().trim().is_empty()
                                    class="shrink-0 rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                                >
                                    {move || if sending.get() { "Sending…" } else { "Send" }}
                                </button>
                            </div>
                        </div>
                    }
                        .into_any()
                } else {
                    view! {
                        <div class="space-y-2">
                            <div class="flex gap-2">
                                <input
                                    class=format!("{input_class} cursor-not-allowed opacity-50")
                                    placeholder="You do not have permission to send messages"
                                    disabled=true
                                />
                                <button
                                    disabled=true
                                    class="shrink-0 cursor-not-allowed rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white opacity-50"
                                >
                                    "Send"
                                </button>
                            </div>
                            <p class="text-xs text-slate-500">
                                {if archived {
                                    "This archived channel is read-only."
                                } else {
                                    "You have read-only access to this chat."
                                }}
                            </p>
                        </div>
                    }
                        .into_any()
                }}
            </div>
        </div>
    }
    .into_any()
}
