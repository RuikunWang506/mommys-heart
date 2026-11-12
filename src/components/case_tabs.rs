use leptos::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseTab {
    Overview,
    Intake,
    Legal,
    ClientCommunications,
    SocialServices,
    CareerBuilding,
    CaseNotes,
    OtherDocuments,
    Activity,
    Closing,
}

impl CaseTab {
    pub const ALL: [CaseTab; 10] = [
        CaseTab::Overview,
        CaseTab::Intake,
        CaseTab::Legal,
        CaseTab::ClientCommunications,
        CaseTab::SocialServices,
        CaseTab::CareerBuilding,
        CaseTab::CaseNotes,
        CaseTab::OtherDocuments,
        CaseTab::Activity,
        CaseTab::Closing,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CaseTab::Overview => "Overview",
            CaseTab::Intake => "Intake",
            CaseTab::Legal => "Legal",
            CaseTab::ClientCommunications => "Client communications",
            CaseTab::SocialServices => "Social services",
            CaseTab::CareerBuilding => "Career building",
            CaseTab::CaseNotes => "Case notes",
            CaseTab::OtherDocuments => "Other documents",
            CaseTab::Activity => "Activity / Change log",
            CaseTab::Closing => "Closing",
        }
    }

    fn slug(self) -> &'static str {
        match self {
            CaseTab::Overview => "overview",
            CaseTab::Intake => "intake",
            CaseTab::Legal => "legal",
            CaseTab::ClientCommunications => "client-communications",
            CaseTab::SocialServices => "social-services",
            CaseTab::CareerBuilding => "career-building",
            CaseTab::CaseNotes => "case-notes",
            CaseTab::OtherDocuments => "other-documents",
            CaseTab::Activity => "activity",
            CaseTab::Closing => "closing",
        }
    }

    /// Folder this tab opens, relative to the case folder; `Some("")` is the case folder itself.
    pub fn folder(self) -> Option<&'static str> {
        match self {
            CaseTab::Intake => Some("Intake"),
            CaseTab::Legal => Some("Supporting Documents/Court Orders"),
            CaseTab::ClientCommunications => Some("Client Emails and Text Messages"),
            CaseTab::SocialServices => Some("Social Services"),
            CaseTab::CaseNotes => Some("Case Notes"),
            CaseTab::OtherDocuments => Some(""),
            CaseTab::Closing => Some("Closing"),
            CaseTab::Overview | CaseTab::CareerBuilding | CaseTab::Activity => None,
        }
    }

    pub fn panel_id() -> &'static str {
        "case-tabpanel"
    }

    pub fn tab_id(self) -> String {
        format!("case-tab-{}", self.slug())
    }
}

#[component]
pub fn CaseTabBar(
    tabs: Vec<CaseTab>,
    active: RwSignal<CaseTab>,
    on_select: Callback<CaseTab>,
    #[prop(into)] badges: Signal<Vec<(CaseTab, usize)>>,
) -> impl IntoView {
    let refs: Vec<NodeRef<leptos::html::Button>> = tabs.iter().map(|_| NodeRef::new()).collect();
    let tabs = StoredValue::new(tabs);
    let refs = StoredValue::new(refs);

    let move_to = move |index: usize| {
        let tab = tabs.with_value(|tabs| tabs[index]);
        on_select.run(tab);
        if let Some(button) = refs.with_value(|refs| refs[index].get()) {
            let _ = button.focus();
        }
    };

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        let count = tabs.with_value(Vec::len);
        let current =
            tabs.with_value(|tabs| tabs.iter().position(|t| *t == active.get_untracked()));
        let Some(current) = current else { return };
        let next = match ev.key().as_str() {
            "ArrowRight" => (current + 1) % count,
            "ArrowLeft" => (current + count - 1) % count,
            "Home" => 0,
            "End" => count - 1,
            _ => return,
        };
        ev.prevent_default();
        move_to(next);
    };

    let buttons = tabs.with_value(|tabs| {
        tabs.iter()
            .enumerate()
            .map(|(index, tab)| {
                let tab = *tab;
                let node_ref = refs.with_value(|refs| refs[index]);
                let selected = move || active.get() == tab;
                view! {
                    <button
                        type="button"
                        role="tab"
                        id=tab.tab_id()
                        node_ref=node_ref
                        aria-selected=move || selected().to_string()
                        aria-controls=CaseTab::panel_id()
                        tabindex=move || if selected() { "0" } else { "-1" }
                        on:click=move |_| on_select.run(tab)
                        class=move || {
                            if selected() {
                                "inline-flex items-center gap-1.5 whitespace-nowrap rounded-full bg-primary-50 px-4 py-2 text-sm font-semibold text-primary-700 ring-1 ring-inset ring-primary-400/40"
                            } else {
                                "inline-flex items-center gap-1.5 whitespace-nowrap rounded-full px-4 py-2 text-sm font-semibold text-slate-400 hover:bg-slate-950 hover:text-slate-200"
                            }
                        }
                    >
                        {tab.label()}
                        {move || {
                            badges
                                .get()
                                .into_iter()
                                .find(|(t, count)| *t == tab && *count > 0)
                                .map(|(_, count)| view! {
                                    <span class="rounded-full bg-amber-500/15 px-[7px] text-xs leading-[19px] font-bold text-amber-200">
                                        {count}
                                    </span>
                                })
                        }}
                    </button>
                }
            })
            .collect_view()
    });

    view! {
        <div
            role="tablist"
            aria-label="Case sections"
            on:keydown=on_keydown
            class="flex gap-1 overflow-x-auto rounded-full border border-slate-800 bg-slate-900 p-1.5"
        >
            {buttons}
        </div>
    }
}
