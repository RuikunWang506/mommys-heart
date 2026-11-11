//! The folder tree every new case starts with — the filing scheme a volunteer
//! opens a case expecting to see.
//!
//! It mirrors the "Sample Client Folder" staff already use in the CLIENTS
//! library, plus an Uploads folder for files that arrive without a type.
//!
//! Like [`new_case_fields`](crate::helpers::new_case_fields), this list lives in
//! code on purpose: it is small, applies org-wide, and changes by deploy rather
//! than at runtime. Adding, renaming, or reordering a folder is a one-line edit
//! here — never a schema change, because a case's folders are ordinary rows.
//!
//! Only the top-level folders name an audience. Everything inside one inherits
//! it, which is the same rule the database enforces: a file's folder is what
//! decides who can see it.

use crate::helpers::visibility::Visibility;

/// One folder a new case starts with, and the folders inside it.
pub struct NewCaseFolder {
    pub name: &'static str,
    /// Who may see what goes in here. The children inherit it.
    pub visibility: Visibility,
    /// Paths below this folder, `/`-separated, parents listed before children.
    pub children: &'static [&'static str],
}

/// Where a client's upload lands when they pick no document type.
pub const UPLOADS_FOLDER: &str = "Uploads";

/// The folders every new case is created with, in display order.
pub const NEW_CASE_FOLDERS: &[NewCaseFolder] = &[
    NewCaseFolder {
        name: "Case Notes",
        // Volunteer-only, because this is where a finalized Case Note's filed
        // document lands. Structured Case Notes are staff-only records, so a
        // shared folder here would hand the client the team's working record.
        visibility: Visibility::VolunteerOnly,
        children: &[],
    },
    NewCaseFolder {
        name: "Client Communications",
        visibility: Visibility::Shared,
        children: &[
            "Emails",
            "Letters or correspondence",
            "Other communications",
            "Text messages",
        ],
    },
    NewCaseFolder {
        name: "Closing",
        visibility: Visibility::VolunteerOnly,
        children: &[
            "Closing note and case closing summary",
            "Final correspondence",
            "Final documents",
        ],
    },
    NewCaseFolder {
        name: "Intake",
        visibility: Visibility::VolunteerOnly,
        children: &[
            "Intake summary",
            "Service agreement and application",
            "Video interview recording",
        ],
    },
    NewCaseFolder {
        name: "Other documents",
        visibility: Visibility::Shared,
        children: &["Miscellaneous and uncategorized"],
    },
    NewCaseFolder {
        name: "Services",
        // Shared so clients can file their own evidence here. Everything below
        // inherits it, including Legal memo and Strategy notes.
        visibility: Visibility::Shared,
        children: &[
            "Career Building",
            "Career Building/Career resources",
            "Career Building/Education and training",
            "Career Building/Employment",
            "Career Building/Job applications and opportunities",
            "Career Building/Other career-building records",
            "Career Building/Referrals",
            "Career Building/Résumé",
            "Legal",
            "Legal/Family and matrimonial law",
            "Legal/Family and matrimonial law/ACS or CPS records",
            "Legal/Family and matrimonial law/Attorney and AFC documents",
            "Legal/Family and matrimonial law/Court documents",
            "Legal/Family and matrimonial law/Court documents/Court orders",
            "Legal/Family and matrimonial law/Court documents/Court transcripts",
            "Legal/Family and matrimonial law/Court documents/Orders of protection",
            "Legal/Family and matrimonial law/Court documents/Petitions and Motions",
            "Legal/Family and matrimonial law/Court documents/Petitions and Motions/Emergency OTSCs",
            "Legal/Family and matrimonial law/Court documents/Petitions and Motions/Motions",
            "Legal/Family and matrimonial law/Court documents/Petitions and Motions/Order to Show Causes",
            "Legal/Family and matrimonial law/Court documents/Petitions and Motions/Petitions",
            "Legal/Family and matrimonial law/Evidence and exhibits",
            "Legal/Family and matrimonial law/Financial records",
            "Legal/Family and matrimonial law/Hospital and medical records",
            "Legal/Family and matrimonial law/Legal memo",
            "Legal/Family and matrimonial law/Mental health evaluations",
            "Legal/Family and matrimonial law/Other legal documents",
            "Legal/Family and matrimonial law/Other relevant documents",
            "Legal/Family and matrimonial law/Police reports",
            "Legal/Family and matrimonial law/Strategy notes",
            "Legal/Immigration law",
            "Legal/Immigration law/Applications and forms",
            "Legal/Immigration law/Identity and civil documents",
            "Legal/Immigration law/Notices and correspondence",
            "Legal/Immigration law/Other immigration documents",
            "Legal/Immigration law/Referrals and follow up",
            "Legal/Immigration law/Supporting documents",
            "Social Services",
            "Social Services/Benefits or public assistance",
            "Social Services/Financial assistance",
            "Social Services/Housing",
            "Social Services/Mental Health and support services",
            "Social Services/Other social-service records",
            "Social Services/Referrals",
            "Supervised Visitation & Safe Exchange",
            "Supervised Visitation & Safe Exchange/Communications and follow up",
            "Supervised Visitation & Safe Exchange/Flyers",
            "Supervised Visitation & Safe Exchange/Incident reports",
            "Supervised Visitation & Safe Exchange/Intake and safety planning",
            "Supervised Visitation & Safe Exchange/Other program documents",
            "Supervised Visitation & Safe Exchange/Scheduling and attendance",
            "Supervised Visitation & Safe Exchange/Service agreements and consents",
            "Supervised Visitation & Safe Exchange/Supervised visitation records",
            "Supervised Visitation & Safe Exchange/Supervised visitation records/Safe exchange records",
        ],
    },
    NewCaseFolder {
        name: UPLOADS_FOLDER,
        visibility: Visibility::Shared,
        children: &[],
    },
];

/// Every folder an upload can be filed into, as case-relative paths, in
/// display order. The picker offers these as document types.
pub fn upload_targets() -> Vec<String> {
    NEW_CASE_FOLDERS
        .iter()
        .flat_map(|spec| {
            std::iter::once(spec.name.to_string())
                .chain(spec.children.iter().map(|child| format!("{}/{child}", spec.name)))
        })
        .collect()
}
