//! Importing people and organizations from a spreadsheet.
//!
//! The shape follows the two tools this sits beside: [`PropertySubject`] chooses
//! People or Organizations exactly as it does for bulk editing, and the task
//! model is the one the contact-mail campaign uses — one at a time, polled,
//! cancellable.
//!
//! The part worth reading is [`MatchKind`]. An import that quietly invents
//! "Physical Location" next to an existing "Location" corrupts the property
//! vocabulary in a way nobody notices for months, so every column is classified
//! against the vocabulary already in use and anything ambiguous is handed back
//! to the person doing the import rather than guessed at.

use leptos::prelude::*;
use leptos::server_fn::codec::{MultipartData, MultipartFormData};
use serde::{Deserialize, Serialize};

use crate::helpers::new_crm_fields::normalize_property_part;
use crate::server_fns::contacts::ContactType;
use crate::server_fns::property_filters::PropertySubject;

/// How many data rows the preview shows under the column names.
pub const SAMPLE_ROWS: usize = 5;
/// How many distinct example values each column card shows.
pub const SAMPLE_VALUES: usize = 3;
/// How many suggestions a flagged column offers.
pub const MAX_SUGGESTIONS: usize = 3;

// ---------------------------------------------------------------------------
// Core fields
// ---------------------------------------------------------------------------

/// A built-in field on the record itself, as opposed to a custom property.
///
/// One enum covers both subjects; [`CoreField::for_subject`] filters it, so a
/// column can never be mapped to a field the chosen subject does not have.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreField {
    // People
    FirstName,
    LastName,
    /// One column holding a whole name, split into first and last on import.
    FullName,
    PreferredName,
    Email,
    Phone,
    Mobile,
    Address,
    JobTitle,
    OrganizationName,
    ContactTypes,
    Source,
    Description,
    DoNotContact,
    // Organizations
    OrgName,
    OrgKind,
    OrgWebsite,
    OrgPhone,
    OrgEmail,
    OrgAddress,
    OrgDescription,
}

const PERSON_FIELDS: &[CoreField] = &[
    CoreField::FirstName,
    CoreField::LastName,
    CoreField::FullName,
    CoreField::PreferredName,
    CoreField::Email,
    CoreField::Phone,
    CoreField::Mobile,
    CoreField::Address,
    CoreField::JobTitle,
    CoreField::OrganizationName,
    CoreField::ContactTypes,
    CoreField::Source,
    CoreField::Description,
    CoreField::DoNotContact,
];

const ORGANIZATION_FIELDS: &[CoreField] = &[
    CoreField::OrgName,
    CoreField::OrgKind,
    CoreField::OrgWebsite,
    CoreField::OrgPhone,
    CoreField::OrgEmail,
    CoreField::OrgAddress,
    CoreField::OrgDescription,
];

impl CoreField {
    pub fn for_subject(subject: PropertySubject) -> &'static [CoreField] {
        match subject {
            PropertySubject::Contact => PERSON_FIELDS,
            PropertySubject::Organization => ORGANIZATION_FIELDS,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::FirstName => "first_name",
            Self::LastName => "last_name",
            Self::FullName => "full_name",
            Self::PreferredName => "preferred_name",
            Self::Email => "email",
            Self::Phone => "phone",
            Self::Mobile => "mobile",
            Self::Address => "address",
            Self::JobTitle => "job_title",
            Self::OrganizationName => "organization_name",
            Self::ContactTypes => "contact_types",
            Self::Source => "source",
            Self::Description => "description",
            Self::DoNotContact => "do_not_contact",
            Self::OrgName => "org_name",
            Self::OrgKind => "org_kind",
            Self::OrgWebsite => "org_website",
            Self::OrgPhone => "org_phone",
            Self::OrgEmail => "org_email",
            Self::OrgAddress => "org_address",
            Self::OrgDescription => "org_description",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::FirstName => "First name",
            Self::LastName => "Last name",
            Self::FullName => "Full name (split into first and last)",
            Self::PreferredName => "Preferred name",
            Self::Email => "Email",
            Self::Phone => "Phone",
            Self::Mobile => "Mobile",
            Self::Address => "Address",
            Self::JobTitle => "Job title",
            Self::OrganizationName => "Organization",
            Self::ContactTypes => "Contact types",
            Self::Source => "Source",
            Self::Description => "Description",
            Self::DoNotContact => "Do not contact",
            Self::OrgName => "Name",
            Self::OrgKind => "Kind",
            Self::OrgWebsite => "Website",
            Self::OrgPhone => "Phone",
            Self::OrgEmail => "Email",
            Self::OrgAddress => "Address",
            Self::OrgDescription => "Description",
        }
    }

    pub fn from_slug(value: &str) -> Option<Self> {
        PERSON_FIELDS
            .iter()
            .chain(ORGANIZATION_FIELDS.iter())
            .copied()
            .find(|field| field.slug() == value)
    }

    /// The header spellings that map to this field without asking.
    ///
    /// Deliberately conservative: a header that is merely *similar* to a core
    /// field is better treated as a property the user can retarget than
    /// silently written into someone's surname.
    fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::FirstName => &["first name", "firstname", "given name", "first"],
            Self::LastName => &["last name", "lastname", "surname", "family name", "last"],
            Self::FullName => &[
                "full name",
                "name",
                "contact name",
                "contact person",
                "contact name (first, last)",
                "contact name (first last)",
                "name (first, last)",
                "name (first last)",
            ],
            Self::PreferredName => &["preferred name", "nickname", "goes by", "known as"],
            Self::Email => &[
                "email",
                "e-mail",
                "email address",
                "e-mail address",
                "mail",
                "contact email",
            ],
            Self::Phone => &[
                "phone",
                "phone number",
                "telephone",
                "work phone",
                "tel",
                "contact phone",
                "contact phone number",
                "contact phone (number)",
            ],
            Self::Mobile => &["mobile", "mobile phone", "cell", "cell phone", "cellphone"],
            Self::Address => &[
                "address",
                "mailing address",
                "street address",
                "postal address",
                "physical address",
                "office address",
                "physical office address",
                "physical (office) address",
            ],
            Self::JobTitle => &[
                "job title",
                "title",
                "position",
                "role",
                "title / role",
                "title/role",
                "role / title",
            ],
            Self::OrganizationName => &[
                "organization",
                "organisation",
                "company",
                "employer",
                "organization name",
                "organisation name",
                "company name",
                "school",
                "college",
                "university",
                "college/university",
                "college / university",
                "agency",
            ],
            Self::ContactTypes => &["contact type", "contact types", "type", "types"],
            Self::Source => &["source", "lead source", "how they found us"],
            Self::Description => &["description", "notes", "note", "comments"],
            Self::DoNotContact => &[
                "do not contact",
                "do-not-contact",
                "dnc",
                "unsubscribed",
                "opted out",
            ],
            Self::OrgName => &[
                "name",
                "organization",
                "organisation",
                "organization name",
                "company",
                "company name",
            ],
            Self::OrgKind => &[
                "kind",
                "type",
                "organization type",
                "organisation type",
                "category",
            ],
            Self::OrgWebsite => &["website", "web site", "url", "web"],
            Self::OrgPhone => &["phone", "phone number", "telephone", "tel"],
            Self::OrgEmail => &["email", "e-mail", "email address"],
            Self::OrgAddress => &["address", "mailing address", "street address"],
            Self::OrgDescription => &["description", "notes", "note", "about"],
        }
    }

    /// Whether this field is the one records are matched on, so the UI can say
    /// so and validation can insist it is mapped.
    pub fn is_match_key(self, subject: PropertySubject) -> bool {
        match subject {
            PropertySubject::Contact => self == Self::Email,
            PropertySubject::Organization => self == Self::OrgName,
        }
    }

    /// Match a header against the alias tables for one subject.
    fn match_header(subject: PropertySubject, header: &str) -> Option<Self> {
        let normalized = normalize_property_part(header);
        Self::for_subject(subject)
            .iter()
            .copied()
            .find(|field| field.aliases().contains(&normalized.as_str()))
    }
}

// ---------------------------------------------------------------------------
// Column mapping
// ---------------------------------------------------------------------------

/// Where one spreadsheet column's values end up.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ColumnTarget {
    #[default]
    Ignore,
    Core {
        field: CoreField,
    },
    Property {
        section: String,
        key: String,
    },
}

impl ColumnTarget {
    pub fn is_ignored(&self) -> bool {
        matches!(self, Self::Ignore)
    }

    /// The property this column writes, trimmed, if it writes one.
    pub fn property(&self) -> Option<(String, String)> {
        match self {
            Self::Property { section, key } => {
                Some((section.trim().to_string(), key.trim().to_string()))
            }
            _ => None,
        }
    }

    /// The core field this column writes, if any.
    pub fn core(&self) -> Option<CoreField> {
        match self {
            Self::Core { field } => Some(*field),
            _ => None,
        }
    }
}

/// Why a column is mapped the way it is. Drives which cards the review panel
/// pins to the top and paints amber.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    /// The header matched a built-in field outright.
    CoreField,
    /// The exact `(section, name)` pair is already in use.
    ExistingProperty,
    /// A property of this name exists, but filed under a different section.
    SectionConflict,
    /// Close to an existing property without being the same. Needs a human.
    SimilarProperty,
    /// Nothing like it exists; importing will create it.
    NewProperty,
    /// Deliberately skipped.
    #[default]
    Ignored,
}

impl MatchKind {
    /// Whether this classification is asking the user a question.
    pub fn needs_review(self) -> bool {
        matches!(self, Self::SectionConflict | Self::SimilarProperty)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::CoreField => "Built-in field",
            Self::ExistingProperty => "Existing property",
            Self::SectionConflict => "Same name, different section",
            Self::SimilarProperty => "Similar to an existing property",
            Self::NewProperty => "New property",
            Self::Ignored => "Not imported",
        }
    }

    pub fn badge_classes(self) -> &'static str {
        match self {
            Self::CoreField => "bg-sky-500/15 text-sky-300",
            Self::ExistingProperty => "bg-emerald-500/15 text-emerald-300",
            Self::SectionConflict | Self::SimilarProperty => "bg-amber-500/15 text-amber-300",
            Self::NewProperty => "bg-primary-500/15 text-primary-300",
            Self::Ignored => "bg-slate-700/40 text-slate-300",
        }
    }
}

/// An existing property offered as the real home for an ambiguous column.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PropertySuggestion {
    pub section: String,
    pub key: String,
    pub usage_count: i64,
    /// Why it is being offered, in words.
    pub reason: String,
}

impl PropertySuggestion {
    pub fn label(&self) -> String {
        format!(
            "{} / {}",
            crate::helpers::sections::label(&self.section),
            self.key
        )
    }
}

/// One column, its chosen target, and everything the user needs to judge it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ColumnPlan {
    pub index: usize,
    pub header: String,
    pub target: ColumnTarget,
    pub match_kind: MatchKind,
    /// One plain sentence describing what this column will actually set.
    pub note: String,
    pub suggestions: Vec<PropertySuggestion>,
    pub sample_values: Vec<String>,
}

/// What the upload step hands back: the file as read, and how it will be read.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportPreview {
    pub upload_id: String,
    pub file_name: String,
    pub sheet_name: String,
    pub row_count: i64,
    pub headers: Vec<String>,
    pub sample_rows: Vec<Vec<String>>,
    /// Title rows above the column names that were skipped, as their text, so
    /// the page can say what it left out.
    #[serde(default)]
    pub skipped_title_rows: Vec<String>,
    pub columns: Vec<ColumnPlan>,
    /// Every `(section, key)` already in use for this subject, so the browser
    /// can offer them without another round trip.
    pub known_properties: Vec<PropertySuggestion>,
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

/// What to do with a row whose record already exists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchAction {
    /// Fill in what the file provides, leaving everything else alone.
    #[default]
    Update,
    /// Leave the record entirely alone.
    Skip,
}

impl MatchAction {
    pub const ALL: &'static [MatchAction] = &[MatchAction::Update, MatchAction::Skip];

    pub fn slug(self) -> &'static str {
        match self {
            Self::Update => "update",
            Self::Skip => "skip",
        }
    }

    pub fn from_slug(value: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|it| it.slug() == value)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Update => "Update the existing record",
            Self::Skip => "Leave the existing record alone",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportPolicy {
    pub on_match: MatchAction,
    /// Whether rows with no existing record create one.
    pub create_missing: bool,
    /// People only: file the contact under the organization named in the file.
    pub link_organization_by_name: bool,
    /// People only: create that organization when it does not exist yet.
    pub create_missing_organizations: bool,
    /// People only: what to file imported people as when the sheet does not say.
    ///
    /// A contact must carry at least one type, and most exported spreadsheets
    /// have no such column, so this is asked once rather than failing every row.
    pub default_contact_type: ContactType,
}

impl Default for ImportPolicy {
    fn default() -> Self {
        Self {
            on_match: MatchAction::Update,
            create_missing: true,
            link_organization_by_name: true,
            create_missing_organizations: false,
            default_contact_type: ContactType::Other,
        }
    }
}

/// The dry run: what the import would do, counted without writing anything.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportPlan {
    pub total_rows: i64,
    pub will_create: i64,
    pub will_update: i64,
    pub will_skip: i64,
    pub invalid: i64,
    /// Properties that do not exist yet and will be created, as "Section / Name".
    pub new_properties: Vec<String>,
    /// Organizations that would be created to file people under.
    pub new_organizations: i64,
    pub needs_review: i64,
    /// Row-level problems, already capped for display.
    pub warnings: Vec<String>,
}

impl ImportPlan {
    /// Whether running this would write anything at all.
    pub fn writes_anything(&self) -> bool {
        self.will_create + self.will_update > 0
    }
}

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportTaskStatus {
    Queued,
    Running,
    Cancelling,
    Completed,
    Cancelled,
    Failed,
}

impl ImportTaskStatus {
    pub fn from_slug(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "running" => Some(Self::Running),
            "cancelling" => Some(Self::Cancelling),
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::Cancelling => "Cancelling",
            Self::Completed => "Completed",
            Self::Cancelled => "Cancelled",
            Self::Failed => "Failed",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running | Self::Cancelling)
    }
}

/// A row that could not be imported, named so it can be found in the file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportRowFailure {
    /// 1-based position among the data rows, matching the spreadsheet.
    pub row: i32,
    pub label: String,
    pub error: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportTask {
    pub id: String,
    pub status: ImportTaskStatus,
    pub subject: PropertySubject,
    pub file_name: String,
    pub created_by_name: String,
    pub row_total: i32,
    pub created_count: i32,
    pub updated_count: i32,
    pub skipped_count: i32,
    pub failed_count: i32,
    pub created_at: String,
    pub started_at: String,
    pub completed_at: String,
    pub cancel_requested_at: String,
    pub cancel_requested_by_name: String,
    pub error: String,
    pub failures: Vec<ImportRowFailure>,
}

impl ImportTask {
    pub fn processed_count(&self) -> i32 {
        self.created_count + self.updated_count + self.skipped_count + self.failed_count
    }

    pub fn remaining_count(&self) -> i32 {
        (self.row_total - self.processed_count()).max(0)
    }

    pub fn percent(&self) -> i32 {
        if self.row_total == 0 {
            return 0;
        }
        (self.processed_count() * 100 / self.row_total).clamp(0, 100)
    }
}

// ---------------------------------------------------------------------------
// Server functions
// ---------------------------------------------------------------------------

/// Every entry point here is an operations/site-admin tool over information-
/// managed data, so both gates apply — the same pair the contact-mail tool uses.
#[cfg(feature = "ssr")]
async fn authorize() -> Result<crate::server_fns::users::User, ServerFnError> {
    let user = crate::server::permissions::require_user().await?;
    crate::server::permissions::require_operations_admin(&user)?;
    crate::server::permissions::require_information_management_access(&user)?;
    Ok(user)
}

/// Turn a repository error into the message the user should see.
///
/// The repository reports a refusal meant for a human as `sqlx::Error::Protocol`;
/// its `Display` wraps that in "encountered unexpected or invalid data: ", which
/// is noise in front of a sentence like "Map one column to Email.". Anything
/// else is a genuine database fault and keeps its own wording.
#[cfg(feature = "ssr")]
fn user_error(error: sqlx::Error) -> ServerFnError {
    match error {
        sqlx::Error::Protocol(message) => ServerFnError::new(message),
        other => ServerFnError::new(other),
    }
}

/// Read an uploaded spreadsheet, stage it, and describe what importing it would
/// mean. Writes nothing to the CRM tables.
#[server(prefix = "/api", input = MultipartFormData)]
pub async fn upload_import_file(data: MultipartData) -> Result<ImportPreview, ServerFnError> {
    let user = authorize().await?;

    let mut multipart = data
        .into_inner()
        .ok_or_else(|| ServerFnError::new("Malformed upload."))?;

    let mut file_name = String::new();
    let mut subject_slug = String::new();
    let mut bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ServerFnError::new(format!("Malformed upload: {e}")))?
    {
        let field_name = field.name().map(str::to_owned);
        let uploaded_name = field.file_name().map(str::to_owned);
        match field_name.as_deref() {
            Some("file") => {
                file_name = uploaded_name.unwrap_or_else(|| "upload".to_string());
                bytes = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| ServerFnError::new(format!("Could not read that file: {e}")))?
                        .to_vec(),
                );
            }
            Some("subject") => subject_slug = field.text().await.unwrap_or_default(),
            _ => {
                let _ = field.bytes().await;
            }
        }
    }

    let subject = PropertySubject::from_slug(subject_slug.trim()).ok_or_else(|| {
        ServerFnError::new("Choose whether this file holds people or organizations.")
    })?;
    let bytes = bytes.ok_or_else(|| ServerFnError::new("Choose a file to import."))?;

    // Strip any path the browser sent; only the extension is actually used.
    let file_name = file_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("upload")
        .to_string();

    let sheet = crate::server::sheets::parse(&file_name, &bytes).map_err(ServerFnError::new)?;

    crate::server::db::crm_import::stage_and_describe(subject, &file_name, sheet, &user.id)
        .await
        .map_err(user_error)
}

/// Count what importing the staged file under this mapping would do.
#[server(prefix = "/api", input = leptos::server_fn::codec::Json)]
pub async fn plan_import(
    upload_id: String,
    subject: PropertySubject,
    columns: Vec<ColumnPlan>,
    policy: ImportPolicy,
) -> Result<ImportPlan, ServerFnError> {
    let user = authorize().await?;
    crate::server::db::crm_import::plan(&upload_id, subject, &columns, &policy, &user.id)
        .await
        .map_err(user_error)
}

/// Begin the import. Refused while another one is still running.
#[server(prefix = "/api", input = leptos::server_fn::codec::Json)]
pub async fn start_import_task(
    upload_id: String,
    subject: PropertySubject,
    columns: Vec<ColumnPlan>,
    policy: ImportPolicy,
) -> Result<ImportTask, ServerFnError> {
    let user = authorize().await?;
    crate::server::crm_import::start(
        &upload_id,
        subject,
        &columns,
        &policy,
        &user.id,
        &user.full_name(),
    )
    .await
    .map_err(ServerFnError::new)
}

/// The running import, or the most recent finished one.
#[server(prefix = "/api")]
pub async fn load_import_task() -> Result<Option<ImportTask>, ServerFnError> {
    authorize().await?;
    if let Some(task) = crate::server::crm_import::current_task().await {
        return Ok(Some(task));
    }
    crate::server::db::crm_import::latest_task()
        .await
        .map_err(user_error)
}

/// Ask the running import to stop. Rows already committed stay committed.
#[server(prefix = "/api")]
pub async fn cancel_import_task(task_id: String) -> Result<ImportTask, ServerFnError> {
    let user = authorize().await?;
    let task_id = task_id.trim();
    if task_id.is_empty() {
        return Err(ServerFnError::new("No import was requested."));
    }
    if let Some(task) = crate::server::crm_import::cancel(task_id, &user.id, &user.full_name())
        .await
        .map_err(ServerFnError::new)?
    {
        return Ok(task);
    }
    crate::server::db::crm_import::latest_task()
        .await
        .map_err(user_error)?
        .filter(|task| task.id == task_id)
        .ok_or_else(|| ServerFnError::new("Import not found."))
}

// ---------------------------------------------------------------------------
// Classification (shared by the upload step and the pre-write revalidation)
// ---------------------------------------------------------------------------

/// Similarity as a 0..=1 ratio from the Levenshtein edit distance.
///
/// Catches the typo case ("Prefered contact method") that token comparison
/// misses, since a misspelling shares no whole word with its correct form.
fn similarity(a: &str, b: &str) -> f32 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    // Two rolling rows rather than the full matrix: property names are short,
    // but this runs for every column against every known property.
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let distance = previous[b.len()];
    1.0 - (distance as f32 / a.len().max(b.len()) as f32)
}

/// Whether `haystack` contains `needle` as a run of whole words.
///
/// This is the "Physical Location" versus "Location" case: the headers are far
/// apart by edit distance but one plainly names the other.
fn contains_words(haystack: &str, needle: &str) -> bool {
    let haystack: Vec<&str> = haystack.split_whitespace().collect();
    let needle: Vec<&str> = needle.split_whitespace().collect();
    if needle.is_empty() || needle.len() >= haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|window| window == needle)
}

/// Shared-word overlap, for headers that reorder or pad a known name.
fn token_overlap(a: &str, b: &str) -> f32 {
    let a: std::collections::HashSet<&str> = a.split_whitespace().collect();
    let b: std::collections::HashSet<&str> = b.split_whitespace().collect();
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(&b).count() as f32;
    let union = a.union(&b).count() as f32;
    intersection / union
}

/// "5 people" / "1 person" — the subject noun, counted.
fn counted_subject(count: i64, subject: PropertySubject) -> String {
    if count == 1 {
        let singular = match subject {
            PropertySubject::Contact => "person",
            PropertySubject::Organization => "organization",
        };
        format!("1 {singular}")
    } else {
        format!("{count} {}", subject.noun_plural())
    }
}

/// Decide where one column should go and why.
///
/// `known` is every `(section, key, usage_count)` already in use for the
/// subject, unioned with the code-owned defaults. The result is advisory: the
/// user's chosen target always wins, and is revalidated before any write.
pub fn classify_column(
    subject: PropertySubject,
    header: &str,
    known: &[PropertySuggestion],
) -> (ColumnTarget, MatchKind, Vec<PropertySuggestion>) {
    if let Some(field) = CoreField::match_header(subject, header) {
        return (ColumnTarget::Core { field }, MatchKind::CoreField, Vec::new());
    }
    classify_property_column(subject, header, known)
}

/// Like [`classify_column`], but only considers properties, never a core field.
pub fn classify_property_column(
    subject: PropertySubject,
    header: &str,
    known: &[PropertySuggestion],
) -> (ColumnTarget, MatchKind, Vec<PropertySuggestion>) {
    let normalized_header = normalize_property_part(header);

    // An exact name match is the property, whatever its section.
    let exact: Vec<&PropertySuggestion> = known
        .iter()
        .filter(|option| normalize_property_part(&option.key) == normalized_header)
        .collect();
    if let Some(best) = exact.first() {
        let target = ColumnTarget::Property {
            section: best.section.clone(),
            key: best.key.clone(),
        };
        // Same name under two sections is exactly the ambiguity worth raising.
        if exact.len() > 1 {
            let suggestions = exact
                .iter()
                .take(MAX_SUGGESTIONS)
                .map(|option| PropertySuggestion {
                    reason: format!(
                        "Filed under {} on {}",
                        crate::helpers::sections::label(&option.section),
                        counted_subject(option.usage_count, subject)
                    ),
                    ..(*option).clone()
                })
                .collect();
            return (target, MatchKind::SectionConflict, suggestions);
        }
        return (target, MatchKind::ExistingProperty, Vec::new());
    }

    // Nothing exact: rank the near misses and let the user decide.
    let mut scored: Vec<(f32, &PropertySuggestion)> = known
        .iter()
        .filter_map(|option| {
            let candidate = normalize_property_part(&option.key);
            let ratio = similarity(&normalized_header, &candidate);
            let overlap = token_overlap(&normalized_header, &candidate);
            let contains = contains_words(&normalized_header, &candidate)
                || contains_words(&candidate, &normalized_header);
            // Containment is the strongest signal, so it is scored above the
            // thresholds the other two have to clear on their own.
            let score = if contains {
                0.95_f32.max(ratio)
            } else if ratio >= 0.82 || overlap >= 0.5 {
                ratio.max(overlap)
            } else {
                return None;
            };
            Some((score, option))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.1.usage_count.cmp(&a.1.usage_count))
    });

    if scored.is_empty() {
        return (
            ColumnTarget::Property {
                // No section is "General"; naming one would be inventing structure.
                section: String::new(),
                key: header.trim().to_string(),
            },
            MatchKind::NewProperty,
            Vec::new(),
        );
    }

    let suggestions: Vec<PropertySuggestion> = scored
        .iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, option)| PropertySuggestion {
            reason: if option.usage_count > 0 {
                format!(
                    "Already used by {}",
                    counted_subject(option.usage_count, subject)
                )
            } else {
                "A standard field, not yet used".to_string()
            },
            ..(*option).clone()
        })
        .collect();

    // Default to keeping the file's own name: adopting a near match without
    // being asked is exactly the silent behaviour this screen exists to avoid.
    (
        ColumnTarget::Property {
            section: String::new(),
            key: header.trim().to_string(),
        },
        MatchKind::SimilarProperty,
        suggestions,
    )
}

/// The plain sentence shown under a column, describing what it will set.
pub fn describe_target(
    subject: PropertySubject,
    target: &ColumnTarget,
    known: &[PropertySuggestion],
) -> String {
    match target {
        ColumnTarget::Ignore => "This column is skipped; nothing is imported from it.".to_string(),
        ColumnTarget::Core { field } if *field == CoreField::FullName => {
            "Splits each name into first and last name: \"Alex Sample\" and \"Sample, Alex\" both become Alex / Sample.".to_string()
        }
        ColumnTarget::Core { field } => {
            if field.is_match_key(subject) {
                format!(
                    "Sets {} \u{2014} also used to tell whether the record already exists.",
                    field.label().to_lowercase()
                )
            } else {
                format!("Sets the built-in {} field.", field.label().to_lowercase())
            }
        }
        ColumnTarget::Property { section, key } => {
            let key = key.trim();
            if key.is_empty() {
                return "Name this property, or skip the column.".to_string();
            }
            let section_label = crate::helpers::sections::label(section);
            let existing = known.iter().find(|option| {
                normalize_property_part(&option.key) == normalize_property_part(key)
                    && normalize_property_part(&option.section) == normalize_property_part(section)
            });
            match existing {
                Some(option) if option.usage_count > 0 => format!(
                    "Sets the existing property \"{key}\" under {section_label}, which {} already have.",
                    counted_subject(option.usage_count, subject)
                ),
                Some(_) => format!("Sets the standard property \"{key}\" under {section_label}."),
                None => format!("Creates a new property \"{key}\" under {section_label}."),
            }
        }
    }
}
