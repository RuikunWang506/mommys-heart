//! Persistence for spreadsheet imports: the staged upload, the task record, and
//! the row-by-row writes into contacts and organizations.
//!
//! The write path deliberately reuses the shapes the ordinary editors use —
//! [`ContactInput`] / [`OrganizationInput`] and their `validate()` — so an
//! imported record is subject to exactly the rules a typed one is. Properties
//! are merged the way [`crate::server::db::bulk_properties::apply`] merges them:
//! an existing row keeps its stored spelling and position and only its value
//! changes, so an import never reorders or re-cases anyone's list.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};

use crate::helpers::new_crm_fields::normalize_property_part;
use crate::server::db::{audit, bulk_properties, ids, pool};
use crate::server::sheets::Sheet;
use crate::server_fns::contact_properties::MAX_PROPERTIES;
use crate::server_fns::contacts::{ContactInput, ContactType};
use crate::server_fns::crm_import::{
    classify_column, describe_target, ColumnPlan, ColumnTarget, CoreField, ImportPlan,
    ImportPolicy, ImportPreview, ImportRowFailure, ImportTask, ImportTaskStatus, MatchAction,
    PropertySuggestion, SAMPLE_ROWS, SAMPLE_VALUES,
};
use crate::server_fns::organizations::{OrganizationInput, OrganizationKind};
use crate::server_fns::property_filters::PropertySubject;

const STAMP: &str = "%Y-%m-%d %H:%M:%S";
/// Staged uploads are scratch space between two steps of one sitting.
const UPLOAD_TTL_HOURS: i64 = 24;
const PURGE_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// How many row problems the dry run lists before summarising.
const MAX_PLAN_WARNINGS: usize = 10;

fn subject_slug(subject: PropertySubject) -> &'static str {
    subject.slug()
}

fn stamp(value: Option<DateTime<Utc>>) -> String {
    value
        .map(|at| at.with_timezone(&chrono::Local).format(STAMP).to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Staging and classification
// ---------------------------------------------------------------------------

/// The parsed sheet, as held between upload and import.
pub struct StagedUpload {
    pub subject: PropertySubject,
    pub file_name: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// The property vocabulary this subject already uses, for classification.
async fn known_properties(
    subject: PropertySubject,
) -> Result<Vec<PropertySuggestion>, sqlx::Error> {
    Ok(bulk_properties::key_options(subject)
        .await?
        .into_iter()
        .map(|option| PropertySuggestion {
            section: option.section,
            key: option.key,
            usage_count: option.usage_count,
            reason: String::new(),
        })
        .collect())
}

/// Store the parsed sheet and describe what importing it would mean.
pub async fn stage_and_describe(
    subject: PropertySubject,
    file_name: &str,
    sheet: Sheet,
    uploaded_by: &str,
) -> Result<ImportPreview, sqlx::Error> {
    let known = known_properties(subject).await?;

    let columns = sheet
        .headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            let (target, match_kind, suggestions) = classify_column(subject, header, &known);
            ColumnPlan {
                index,
                header: header.clone(),
                note: describe_target(subject, &target, &known),
                target,
                match_kind,
                suggestions,
                sample_values: sample_values(&sheet.rows, index),
            }
        })
        .collect();

    let id = ids::opaque("imp");
    sqlx::query(
        "INSERT INTO crm_import_uploads
            (id, subject, file_name, sheet_name, headers, rows, row_count, uploaded_by_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(&id)
    .bind(subject_slug(subject))
    .bind(file_name)
    .bind(&sheet.sheet_name)
    .bind(serde_json::to_value(&sheet.headers).unwrap_or_default())
    .bind(serde_json::to_value(&sheet.rows).unwrap_or_default())
    .bind(sheet.rows.len() as i32)
    .bind(uploaded_by)
    .execute(pool())
    .await?;

    Ok(ImportPreview {
        upload_id: id,
        file_name: file_name.to_string(),
        sheet_name: sheet.sheet_name.clone(),
        row_count: sheet.rows.len() as i64,
        sample_rows: sheet.rows.iter().take(SAMPLE_ROWS).cloned().collect(),
        skipped_title_rows: sheet.skipped_title_rows.clone(),
        headers: sheet.headers,
        columns,
        known_properties: known,
    })
}

/// A few distinct non-empty values from one column, for context on its card.
fn sample_values(rows: &[Vec<String>], index: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    rows.iter()
        .filter_map(|row| row.get(index))
        .filter(|value| !value.trim().is_empty())
        .filter(|value| seen.insert(value.to_lowercase()))
        .take(SAMPLE_VALUES)
        .cloned()
        .collect()
}

/// Load a staged upload, refusing one belonging to somebody else.
pub async fn load_upload(
    upload_id: &str,
    uploaded_by: &str,
) -> Result<StagedUpload, sqlx::Error> {
    let row = sqlx::query_as::<_, (String, String, serde_json::Value, serde_json::Value)>(
        "SELECT subject, file_name, headers, rows FROM crm_import_uploads
         WHERE id = $1 AND uploaded_by_id = $2",
    )
    .bind(upload_id)
    .bind(uploaded_by)
    .fetch_optional(pool())
    .await?
    .ok_or_else(|| {
        sqlx::Error::Protocol(
            "That upload is no longer available. Choose the file again.".into(),
        )
    })?;

    let subject = PropertySubject::from_slug(&row.0)
        .ok_or_else(|| sqlx::Error::Protocol("Unknown import subject.".into()))?;
    Ok(StagedUpload {
        subject,
        file_name: row.1,
        headers: serde_json::from_value(row.2).unwrap_or_default(),
        rows: serde_json::from_value(row.3).unwrap_or_default(),
    })
}

/// Drop staged uploads nobody came back to finish.
pub async fn purge_expired_uploads() -> Result<u64, sqlx::Error> {
    let result = sqlx::query(&format!(
        "DELETE FROM crm_import_uploads WHERE created_at < now() - interval '{UPLOAD_TTL_HOURS} hours'"
    ))
    .execute(pool())
    .await?;
    Ok(result.rows_affected())
}

/// Periodically drop expired staged uploads, mirroring the other retention
/// tasks: failures are logged and never crash the server.
pub fn start_retention_task() {
    tokio::spawn(async {
        loop {
            match purge_expired_uploads().await {
                Ok(n) if n > 0 => tracing::info!("import retention: deleted {n} staged uploads"),
                Ok(_) => {}
                Err(e) => tracing::warn!("import retention failed: {e}"),
            }
            tokio::time::sleep(PURGE_INTERVAL).await;
        }
    });
}

// ---------------------------------------------------------------------------
// Mapping
// ---------------------------------------------------------------------------

/// A column mapping validated against the staged sheet.
///
/// Built once per plan or import so the per-row loop never re-parses the
/// mapping, and so a target that cannot be honoured is rejected before any row
/// is touched rather than failing halfway through.
pub struct Mapping {
    core: Vec<(usize, CoreField)>,
    properties: Vec<(usize, String, String)>,
}

impl Mapping {
    /// Validate the browser's chosen targets against the sheet and the subject.
    ///
    /// The user's choice always wins over the suggestion, but it still has to be
    /// a target this subject actually has and a property name that fits.
    pub fn build(
        subject: PropertySubject,
        headers: &[String],
        columns: &[ColumnPlan],
    ) -> Result<Self, String> {
        use crate::server_fns::contact_properties::MAX_KEY_CHARS;

        let mut core = Vec::new();
        let mut properties = Vec::new();
        let mut seen_core = HashSet::new();
        let mut seen_property = HashSet::new();

        for column in columns {
            if column.index >= headers.len() {
                return Err("That mapping does not match the file. Upload it again.".into());
            }
            match &column.target {
                ColumnTarget::Ignore => {}
                ColumnTarget::Core { field } => {
                    if !CoreField::for_subject(subject).contains(field) {
                        return Err(format!(
                            "\"{}\" cannot be mapped to {} for {}.",
                            column.header,
                            field.label(),
                            subject.noun_plural()
                        ));
                    }
                    // Two columns writing one field would make the winner depend
                    // on column order, which is not a thing anyone can predict.
                    if !seen_core.insert(*field) {
                        return Err(format!(
                            "Two columns are both mapped to {}. Pick one.",
                            field.label()
                        ));
                    }
                    core.push((column.index, *field));
                }
                ColumnTarget::Property { section, key } => {
                    let section = section.trim().to_string();
                    let key = key.trim().to_string();
                    if key.is_empty() {
                        return Err(format!(
                            "Name the property for \"{}\", or skip that column.",
                            column.header
                        ));
                    }
                    if key.chars().count() > MAX_KEY_CHARS {
                        return Err(format!(
                            "The property name for \"{}\" must be {MAX_KEY_CHARS} characters or fewer.",
                            column.header
                        ));
                    }
                    let normalized = (
                        normalize_property_part(&section),
                        normalize_property_part(&key),
                    );
                    if !seen_property.insert(normalized) {
                        return Err(format!(
                            "Two columns are both mapped to the property \"{key}\". Pick one."
                        ));
                    }
                    properties.push((column.index, section, key));
                }
            }
        }

        if core.is_empty() && properties.is_empty() {
            return Err("Every column is set to be skipped, so there is nothing to import.".into());
        }

        // A whole-name column and separate name columns would disagree about
        // whose value wins.
        if seen_core.contains(&CoreField::FullName)
            && (seen_core.contains(&CoreField::FirstName)
                || seen_core.contains(&CoreField::LastName))
        {
            return Err(
                "Map names either to Full name or to First name and Last name, not both.".into(),
            );
        }

        let match_key = match subject {
            PropertySubject::Contact => CoreField::Email,
            PropertySubject::Organization => CoreField::OrgName,
        };
        if !seen_core.contains(&match_key) {
            return Err(match subject {
                PropertySubject::Contact => {
                    "Map one column to Email. It is how an import tells an existing person from a new one.".into()
                }
                PropertySubject::Organization => {
                    "Map one column to Name. It is how an import tells an existing organization from a new one.".into()
                }
            });
        }
        Ok(Self { core, properties })
    }

    fn cell<'a>(&self, row: &'a [String], index: usize) -> &'a str {
        row.get(index).map(String::as_str).unwrap_or_default()
    }

    fn core_value<'a>(&self, row: &'a [String], field: CoreField) -> &'a str {
        let direct = self
            .core
            .iter()
            .find(|(_, mapped)| *mapped == field)
            .map(|(index, _)| self.cell(row, *index));
        if let Some(value) = direct {
            return value;
        }
        // First and last name can come from a single whole-name column.
        if matches!(field, CoreField::FirstName | CoreField::LastName) {
            if let Some((index, _)) = self
                .core
                .iter()
                .find(|(_, mapped)| *mapped == CoreField::FullName)
            {
                let (first, last) = split_full_name(self.cell(row, *index));
                return if field == CoreField::FirstName {
                    first
                } else {
                    last
                };
            }
        }
        ""
    }

    /// The `(section, key, value)` triples this row writes, skipping blanks.
    ///
    /// A blank cell means "this file has nothing to say about that property",
    /// not "erase it" — an import must never quietly empty a field somebody
    /// filled in by hand.
    fn property_values(&self, row: &[String]) -> Vec<(String, String, String)> {
        self.properties
            .iter()
            .filter_map(|(index, section, key)| {
                let value = self.cell(row, *index).trim();
                (!value.is_empty()).then(|| {
                    (section.clone(), key.clone(), value.to_string())
                })
            })
            .collect()
    }
}

/// "Yes" in the spellings a spreadsheet actually contains.
fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_lowercase().as_str(),
        "yes" | "y" | "true" | "t" | "1" | "x" | "on"
    )
}

/// Contact types named in a cell, ignoring anything unrecognised.
fn parse_types_raw(value: &str) -> Vec<ContactType> {
    let mut types: Vec<ContactType> = value
        .split([',', ';', '|', '/'])
        .filter_map(|part| {
            let normalized = normalize_property_part(part);
            ContactType::ALL.iter().copied().find(|kind| {
                kind.slug() == normalized.replace(' ', "_")
                    || normalize_property_part(kind.label()) == normalized
            })
        })
        .collect();
    types.dedup();
    types.truncate(crate::server_fns::contacts::MAX_TYPES);
    types
}

/// Contact types for a *new* contact, falling back to the policy's default.
///
/// A contact must carry at least one type and most spreadsheets have no such
/// column, so the fallback is what keeps an otherwise good row importable. The
/// update path deliberately does not use this: overwriting somebody's existing
/// types with a default just because the file was silent would lose data.
fn parse_types(value: &str, fallback: ContactType) -> Vec<ContactType> {
    let mut types = parse_types_raw(value);
    if types.is_empty() {
        types.push(fallback);
    }
    types
}

fn parse_kind(value: &str) -> OrganizationKind {
    let normalized = normalize_property_part(value);
    OrganizationKind::ALL
        .iter()
        .copied()
        .find(|kind| {
            kind.slug() == normalized.replace(' ', "_")
                || normalize_property_part(kind.label()) == normalized
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Dry run
// ---------------------------------------------------------------------------

/// Count what an import would do, touching nothing.
pub async fn plan(
    upload_id: &str,
    subject: PropertySubject,
    columns: &[ColumnPlan],
    policy: &ImportPolicy,
    user_id: &str,
) -> Result<ImportPlan, sqlx::Error> {
    let upload = load_upload(upload_id, user_id).await?;
    if upload.subject != subject {
        return Err(sqlx::Error::Protocol(
            "That file was uploaded for the other record type. Upload it again.".into(),
        ));
    }
    let mapping =
        Mapping::build(subject, &upload.headers, columns).map_err(sqlx::Error::Protocol)?;

    let mut plan = ImportPlan {
        total_rows: upload.rows.len() as i64,
        needs_review: columns
            .iter()
            .filter(|column| column.match_kind.needs_review() && !column.target.is_ignored())
            .count() as i64,
        ..Default::default()
    };

    // Resolve every key in one query rather than per row: a 5000-row file would
    // otherwise be 5000 round trips just to count.
    let keys: Vec<String> = upload
        .rows
        .iter()
        .map(|row| match_key_of(subject, &mapping, row))
        .collect();
    let existing = existing_ids(subject, &keys).await?;

    let mut new_properties: Vec<String> = Vec::new();
    let known = known_properties(subject).await?;
    let known_set: HashSet<(String, String)> = known
        .iter()
        .map(|option| {
            (
                normalize_property_part(&option.section),
                normalize_property_part(&option.key),
            )
        })
        .collect();
    for (_, section, key) in &mapping.properties {
        let normalized = (
            normalize_property_part(section),
            normalize_property_part(key),
        );
        if !known_set.contains(&normalized) {
            new_properties.push(format!(
                "{} / {key}",
                crate::helpers::sections::label(section)
            ));
        }
    }
    new_properties.sort();
    new_properties.dedup();
    plan.new_properties = new_properties;

    // Organizations named across the file that do not exist yet.
    let mut wanted_organizations: HashSet<String> = HashSet::new();
    if subject == PropertySubject::Contact && policy.link_organization_by_name {
        let names: Vec<String> = upload
            .rows
            .iter()
            .map(|row| {
                mapping
                    .core_value(row, CoreField::OrganizationName)
                    .trim()
                    .to_string()
            })
            .filter(|name| !name.is_empty())
            .collect();
        let found = organization_ids_by_name(&names).await?;
        for name in names {
            if !found.contains_key(&normalize_property_part(&name)) {
                wanted_organizations.insert(normalize_property_part(&name));
            }
        }
    }
    plan.new_organizations = if policy.create_missing_organizations {
        wanted_organizations.len() as i64
    } else {
        0
    };

    // A key repeated inside one file resolves to the same record, so only the
    // first occurrence can create it; the rest are updates.
    let mut seen_keys: HashSet<String> = HashSet::new();

    for (position, row) in upload.rows.iter().enumerate() {
        let key = match_key_of(subject, &mapping, row);
        if key.trim().is_empty() {
            plan.invalid += 1;
            push_warning(
                &mut plan.warnings,
                position + 1,
                match subject {
                    PropertySubject::Contact => "no email address",
                    PropertySubject::Organization => "no name",
                },
            );
            continue;
        }
        let normalized = normalize_property_part(&key);
        let already = existing.contains_key(&normalized) || seen_keys.contains(&normalized);

        if already {
            match policy.on_match {
                MatchAction::Update => plan.will_update += 1,
                MatchAction::Skip => plan.will_skip += 1,
            }
        } else if policy.create_missing {
            // A row that cannot be built is counted here rather than at write
            // time, so the number on screen is the number that will happen.
            if let Err(error) =
                validate_new_record(subject, &mapping, row, policy, &wanted_organizations)
            {
                plan.invalid += 1;
                push_warning(&mut plan.warnings, position + 1, &error);
                continue;
            }
            plan.will_create += 1;
            seen_keys.insert(normalized);
        } else {
            plan.will_skip += 1;
        }
    }

    Ok(plan)
}

fn push_warning(warnings: &mut Vec<String>, row: usize, detail: &str) {
    if warnings.len() < MAX_PLAN_WARNINGS {
        warnings.push(format!("Row {row}: {detail}"));
    }
}

/// The value a row is matched on: email for people, name for organizations.
fn match_key_of(subject: PropertySubject, mapping: &Mapping, row: &[String]) -> String {
    match subject {
        PropertySubject::Contact => mapping.core_value(row, CoreField::Email).trim().to_string(),
        PropertySubject::Organization => {
            mapping.core_value(row, CoreField::OrgName).trim().to_string()
        }
    }
}

/// Existing record ids for a batch of match keys, indexed by normalized key.
async fn existing_ids(
    subject: PropertySubject,
    keys: &[String],
) -> Result<HashMap<String, String>, sqlx::Error> {
    let normalized: Vec<String> = keys
        .iter()
        .map(|key| normalize_property_part(key))
        .filter(|key| !key.is_empty())
        .collect();
    if normalized.is_empty() {
        return Ok(HashMap::new());
    }
    // The SQL normalization has to agree with `normalize_property_part`:
    // lowercase and collapse internal whitespace.
    let sql = match subject {
        PropertySubject::Contact => {
            "SELECT lower(regexp_replace(btrim(email), '\\s+', ' ', 'g')), id
             FROM contacts
             WHERE lower(regexp_replace(btrim(email), '\\s+', ' ', 'g')) = ANY($1::text[])
             ORDER BY archived ASC, id ASC"
        }
        PropertySubject::Organization => {
            "SELECT lower(regexp_replace(btrim(name), '\\s+', ' ', 'g')), id
             FROM organizations
             WHERE lower(regexp_replace(btrim(name), '\\s+', ' ', 'g')) = ANY($1::text[])
             ORDER BY archived ASC, id ASC"
        }
    };
    let rows = sqlx::query_as::<_, (String, String)>(sql)
        .bind(&normalized)
        .fetch_all(pool())
        .await?;
    // First wins: the ORDER BY puts a live record ahead of an archived one.
    let mut found = HashMap::new();
    for (key, id) in rows {
        found.entry(key).or_insert(id);
    }
    Ok(found)
}

async fn organization_ids_by_name(
    names: &[String],
) -> Result<HashMap<String, String>, sqlx::Error> {
    existing_ids(PropertySubject::Organization, names).await
}

/// Validate the record a row would create, without keeping it.
///
/// Shared by the dry run and the write path's own construction, so "invalid"
/// means the same thing in the preview as it does in the outcome.
fn validate_new_record(
    subject: PropertySubject,
    mapping: &Mapping,
    row: &[String],
    policy: &ImportPolicy,
    creatable_organizations: &HashSet<String>,
) -> Result<(), String> {
    match subject {
        PropertySubject::Contact => {
            let organization_name = mapping.core_value(row, CoreField::OrganizationName).trim();
            // The id is filled in at write time; validation only needs to know
            // whether there will be one, since "no surname and no employer" is
            // the one combination the contacts table rejects.
            let will_have_organization = policy.link_organization_by_name
                && !organization_name.is_empty()
                && (policy.create_missing_organizations
                    || !creatable_organizations.contains(&normalize_property_part(organization_name)));
            let input = ContactInput {
                first_name: mapping.core_value(row, CoreField::FirstName).to_string(),
                last_name: mapping.core_value(row, CoreField::LastName).to_string(),
                preferred_name: mapping.core_value(row, CoreField::PreferredName).to_string(),
                email: mapping.core_value(row, CoreField::Email).to_string(),
                phone: mapping.core_value(row, CoreField::Phone).to_string(),
                mobile: mapping.core_value(row, CoreField::Mobile).to_string(),
                address: mapping.core_value(row, CoreField::Address).to_string(),
                job_title: mapping.core_value(row, CoreField::JobTitle).to_string(),
                organization_id: if will_have_organization {
                    // A placeholder that only has to be non-empty for validation.
                    "pending".to_string()
                } else {
                    String::new()
                },
                types: parse_types(
                    mapping.core_value(row, CoreField::ContactTypes),
                    policy.default_contact_type,
                ),
                source: mapping.core_value(row, CoreField::Source).to_string(),
                description: mapping.core_value(row, CoreField::Description).to_string(),
                do_not_contact: parse_bool(mapping.core_value(row, CoreField::DoNotContact)),
            };
            input.validate()?;
            Ok(())
        }
        PropertySubject::Organization => {
            let input = OrganizationInput {
                name: mapping.core_value(row, CoreField::OrgName).to_string(),
                kind: parse_kind(mapping.core_value(row, CoreField::OrgKind)),
                website: mapping.core_value(row, CoreField::OrgWebsite).to_string(),
                phone: mapping.core_value(row, CoreField::OrgPhone).to_string(),
                email: mapping.core_value(row, CoreField::OrgEmail).to_string(),
                address: mapping.core_value(row, CoreField::OrgAddress).to_string(),
                description: mapping.core_value(row, CoreField::OrgDescription).to_string(),
            };
            input.validate()?;
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Task lifecycle
// ---------------------------------------------------------------------------

/// The in-memory task, with the staged rows it is working through.
#[derive(Clone)]
pub struct ImportTaskRecord {
    pub id: String,
    pub status: ImportTaskStatus,
    pub subject: PropertySubject,
    pub file_name: String,
    pub created_by_id: String,
    pub created_by_name: String,
    pub row_total: i32,
    pub created_count: i32,
    pub updated_count: i32,
    pub skipped_count: i32,
    pub failed_count: i32,
    pub created_at: DateTime<Utc>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub cancel_requested_at: Option<DateTime<Utc>>,
    pub cancel_requested_by_id: Option<String>,
    pub cancel_requested_by_name: String,
    pub error: String,
    pub failures: Vec<ImportRowFailure>,
}

impl ImportTaskRecord {
    pub fn to_public(&self) -> ImportTask {
        ImportTask {
            id: self.id.clone(),
            status: self.status,
            subject: self.subject,
            file_name: self.file_name.clone(),
            created_by_name: self.created_by_name.clone(),
            row_total: self.row_total,
            created_count: self.created_count,
            updated_count: self.updated_count,
            skipped_count: self.skipped_count,
            failed_count: self.failed_count,
            created_at: stamp(Some(self.created_at)),
            started_at: stamp(Some(self.started_at)),
            completed_at: stamp(self.completed_at),
            cancel_requested_at: stamp(self.cancel_requested_at),
            cancel_requested_by_name: self.cancel_requested_by_name.clone(),
            error: self.error.clone(),
            failures: self.failures.clone(),
        }
    }
}

/// Everything the runner needs, resolved and validated up front.
pub struct PreparedImport {
    pub record: ImportTaskRecord,
    pub rows: Vec<Vec<String>>,
    pub mapping: Mapping,
    pub policy: ImportPolicy,
}

/// Validate the request, write the task row, and hand back a runnable import.
///
/// Everything that can be refused is refused here, before the task exists, so a
/// bad mapping never produces a half-finished import in the history.
pub async fn start_task(
    upload_id: &str,
    subject: PropertySubject,
    columns: &[ColumnPlan],
    policy: &ImportPolicy,
    creator_id: &str,
    creator_name: &str,
) -> Result<PreparedImport, sqlx::Error> {
    let upload = load_upload(upload_id, creator_id).await?;
    if upload.subject != subject {
        return Err(sqlx::Error::Protocol(
            "That file was uploaded for the other record type. Upload it again.".into(),
        ));
    }
    let mapping =
        Mapping::build(subject, &upload.headers, columns).map_err(sqlx::Error::Protocol)?;
    if upload.rows.is_empty() {
        return Err(sqlx::Error::Protocol("That file has no rows to import.".into()));
    }

    let id = ids::opaque("imptask");
    let now = Utc::now();
    let mut tx = pool().begin().await?;
    sqlx::query(
        "INSERT INTO crm_import_tasks
            (id, status, subject, file_name, mapping, policy, created_by_id, created_by_name,
             row_total, started_at)
         VALUES ($1, 'running', $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(&id)
    .bind(subject_slug(subject))
    .bind(&upload.file_name)
    .bind(serde_json::to_value(columns).unwrap_or_default())
    .bind(serde_json::to_value(policy).unwrap_or_default())
    .bind(creator_id)
    .bind(creator_name)
    .bind(upload.rows.len() as i32)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // One statement for all rows: a 5000-row file would otherwise be 5000
    // inserts before the import has done any actual work.
    let positions: Vec<i32> = (1..=upload.rows.len() as i32).collect();
    sqlx::query(
        "INSERT INTO crm_import_rows (task_id, position)
         SELECT $1, position FROM unnest($2::int[]) AS position",
    )
    .bind(&id)
    .bind(&positions)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    // The staged grid has been copied into the task; nothing needs it again.
    let _ = sqlx::query("DELETE FROM crm_import_uploads WHERE id = $1")
        .bind(upload_id)
        .execute(pool())
        .await;

    Ok(PreparedImport {
        record: ImportTaskRecord {
            id,
            status: ImportTaskStatus::Running,
            subject,
            file_name: upload.file_name,
            created_by_id: creator_id.to_string(),
            created_by_name: creator_name.to_string(),
            row_total: upload.rows.len() as i32,
            created_count: 0,
            updated_count: 0,
            skipped_count: 0,
            failed_count: 0,
            created_at: now,
            started_at: now,
            completed_at: None,
            cancel_requested_at: None,
            cancel_requested_by_id: None,
            cancel_requested_by_name: String::new(),
            error: String::new(),
            failures: Vec::new(),
        },
        rows: upload.rows,
        mapping,
        policy: policy.clone(),
    })
}

/// What happened to one source row.
pub enum RowOutcome {
    Created(String),
    Updated(String),
    Skipped,
    Failed(String),
}

impl RowOutcome {
    fn status(&self) -> &'static str {
        match self {
            Self::Created(_) => "created",
            Self::Updated(_) => "updated",
            Self::Skipped => "skipped",
            Self::Failed(_) => "failed",
        }
    }
}

/// Record one row's outcome, so progress survives a crash mid-import.
pub async fn record_row(
    task_id: &str,
    position: i32,
    outcome: &RowOutcome,
    label: &str,
) -> Result<(), sqlx::Error> {
    let (record_id, error) = match outcome {
        RowOutcome::Created(id) | RowOutcome::Updated(id) => (id.as_str(), ""),
        RowOutcome::Skipped => ("", ""),
        RowOutcome::Failed(error) => ("", error.as_str()),
    };
    sqlx::query(
        "UPDATE crm_import_rows SET status = $3, record_id = $4, label = $5, error = $6
         WHERE task_id = $1 AND position = $2",
    )
    .bind(task_id)
    .bind(position)
    .bind(outcome.status())
    .bind(record_id)
    .bind(label)
    .bind(error)
    .execute(pool())
    .await?;
    Ok(())
}

/// Commit the terminal snapshot of a finished import.
pub async fn finish_task(task: &ImportTaskRecord) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE crm_import_tasks
         SET status = $2, created_count = $3, updated_count = $4, skipped_count = $5,
             failed_count = $6, completed_at = $7, cancel_requested_at = $8,
             cancel_requested_by_id = $9, cancel_requested_by_name = $10, error = $11
         WHERE id = $1",
    )
    .bind(&task.id)
    .bind(task.status.slug())
    .bind(task.created_count)
    .bind(task.updated_count)
    .bind(task.skipped_count)
    .bind(task.failed_count)
    .bind(task.completed_at)
    .bind(task.cancel_requested_at)
    .bind(&task.cancel_requested_by_id)
    .bind(&task.cancel_requested_by_name)
    .bind(&task.error)
    .execute(pool())
    .await?;
    Ok(())
}

/// Close imports left open by a process stop. Rows already written stay written;
/// only the task's own bookkeeping is reconciled from what the rows say.
pub async fn fail_interrupted_tasks() -> Result<(), sqlx::Error> {
    let mut tx = pool().begin().await?;
    sqlx::query(
        "UPDATE crm_import_rows row
         SET status = 'failed', error = $1
         FROM crm_import_tasks task
         WHERE row.task_id = task.id
           AND task.status IN ('queued', 'running', 'cancelling')
           AND row.status = 'pending'",
    )
    .bind("The server stopped before this row was imported.")
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE crm_import_tasks task
         SET status = 'failed', completed_at = now(),
             created_count = outcomes.created_count,
             updated_count = outcomes.updated_count,
             skipped_count = outcomes.skipped_count,
             failed_count = outcomes.failed_count,
             error = $1
         FROM (
             SELECT task_id,
                    count(*) FILTER (WHERE status = 'created')::integer AS created_count,
                    count(*) FILTER (WHERE status = 'updated')::integer AS updated_count,
                    count(*) FILTER (WHERE status = 'skipped')::integer AS skipped_count,
                    count(*) FILTER (WHERE status = 'failed')::integer AS failed_count
             FROM crm_import_rows GROUP BY task_id
         ) outcomes
         WHERE task.id = outcomes.task_id
           AND task.status IN ('queued', 'running', 'cancelling')",
    )
    .bind("The server stopped before this import finished. Records already imported were kept.")
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

#[derive(sqlx::FromRow)]
struct TaskRow {
    id: String,
    status: String,
    subject: String,
    file_name: String,
    created_by_name: String,
    row_total: i32,
    created_count: i32,
    updated_count: i32,
    skipped_count: i32,
    failed_count: i32,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    cancel_requested_at: Option<DateTime<Utc>>,
    cancel_requested_by_name: String,
    error: String,
}

/// The most recent import, for the panel to show when nothing is running.
pub async fn latest_task() -> Result<Option<ImportTask>, sqlx::Error> {
    let Some(row) = sqlx::query_as::<_, TaskRow>(
        "SELECT id, status, subject, file_name, created_by_name, row_total,
                created_count, updated_count, skipped_count, failed_count,
                created_at, started_at, completed_at, cancel_requested_at,
                cancel_requested_by_name, error
         FROM crm_import_tasks ORDER BY seq DESC LIMIT 1",
    )
    .fetch_optional(pool())
    .await?
    else {
        return Ok(None);
    };

    let status = ImportTaskStatus::from_slug(&row.status)
        .ok_or_else(|| sqlx::Error::Protocol("Unknown import status.".into()))?;
    let subject = PropertySubject::from_slug(&row.subject)
        .ok_or_else(|| sqlx::Error::Protocol("Unknown import subject.".into()))?;
    let failures = sqlx::query_as::<_, (i32, String, String)>(
        "SELECT position, label, error FROM crm_import_rows
         WHERE task_id = $1 AND status = 'failed' ORDER BY position LIMIT 200",
    )
    .bind(&row.id)
    .fetch_all(pool())
    .await?
    .into_iter()
    .map(|(row_number, label, error)| ImportRowFailure {
        row: row_number,
        label,
        error,
    })
    .collect();

    Ok(Some(ImportTask {
        id: row.id,
        status,
        subject,
        file_name: row.file_name,
        created_by_name: row.created_by_name,
        row_total: row.row_total,
        created_count: row.created_count,
        updated_count: row.updated_count,
        skipped_count: row.skipped_count,
        failed_count: row.failed_count,
        created_at: stamp(Some(row.created_at)),
        started_at: stamp(row.started_at),
        completed_at: stamp(row.completed_at),
        cancel_requested_at: stamp(row.cancel_requested_at),
        cancel_requested_by_name: row.cancel_requested_by_name,
        error: row.error,
        failures,
    }))
}

// ---------------------------------------------------------------------------
// The write
// ---------------------------------------------------------------------------

/// Import one row in its own transaction.
///
/// Per-row rather than one big transaction so a single bad row cannot roll back
/// an hour of good work, and so the progress the panel polls is real.
pub async fn import_row(
    subject: PropertySubject,
    mapping: &Mapping,
    policy: &ImportPolicy,
    row: &[String],
    actor_user_id: &str,
    actor: &str,
) -> RowOutcome {
    match import_row_inner(subject, mapping, policy, row, actor_user_id, actor).await {
        Ok(outcome) => outcome,
        Err(error) => RowOutcome::Failed(error),
    }
}

/// Split a whole name into `(first, last)`, borrowing from the cell.
///
/// "Last, First" (with a comma) is read surname-first; otherwise the last word
/// is the surname and everything before it the first name. A single word is
/// treated as the surname, since a person record needs one.
fn split_full_name(value: &str) -> (&str, &str) {
    let value = value.trim();
    if let Some((last, first)) = value.split_once(',') {
        let (first, last) = (first.trim(), last.trim());
        if !first.is_empty() && !last.is_empty() {
            return (first, last);
        }
    }
    match value.rsplit_once(char::is_whitespace) {
        Some((first, last)) => (first.trim(), last.trim()),
        None => ("", value),
    }
}

/// A short human label for a row, so a failure can be found in the file.
pub fn row_label(subject: PropertySubject, mapping: &Mapping, row: &[String]) -> String {
    match subject {
        PropertySubject::Contact => {
            let name = format!(
                "{} {}",
                mapping.core_value(row, CoreField::FirstName),
                mapping.core_value(row, CoreField::LastName)
            );
            let name = name.trim();
            if name.is_empty() {
                mapping.core_value(row, CoreField::Email).to_string()
            } else {
                name.to_string()
            }
        }
        PropertySubject::Organization => mapping.core_value(row, CoreField::OrgName).to_string(),
    }
}

async fn import_row_inner(
    subject: PropertySubject,
    mapping: &Mapping,
    policy: &ImportPolicy,
    row: &[String],
    actor_user_id: &str,
    actor: &str,
) -> Result<RowOutcome, String> {
    let key = match_key_of(subject, mapping, row);
    if key.trim().is_empty() {
        return Err(match subject {
            PropertySubject::Contact => "No email address, so this row cannot be matched or created.",
            PropertySubject::Organization => "No name, so this row cannot be matched or created.",
        }
        .to_string());
    }

    let existing = existing_ids(subject, std::slice::from_ref(&key))
        .await
        .map_err(|e| e.to_string())?
        .get(&normalize_property_part(&key))
        .cloned();

    if existing.is_some() && policy.on_match == MatchAction::Skip {
        return Ok(RowOutcome::Skipped);
    }
    if existing.is_none() && !policy.create_missing {
        return Ok(RowOutcome::Skipped);
    }

    let mut tx = pool().begin().await.map_err(|e| e.to_string())?;
    audit::set_actor_in_transaction(&mut tx, actor_user_id)
        .await
        .map_err(|e| e.to_string())?;

    let outcome = match subject {
        PropertySubject::Contact => {
            write_contact(&mut tx, mapping, policy, row, existing, actor).await?
        }
        PropertySubject::Organization => {
            write_organization(&mut tx, mapping, row, existing, actor).await?
        }
    };

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(outcome)
}

/// Resolve the organization a contact row names, creating it if allowed.
async fn resolve_organization(
    tx: &mut Transaction<'_, Postgres>,
    name: &str,
    policy: &ImportPolicy,
    actor: &str,
) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || !policy.link_organization_by_name {
        return Ok(String::new());
    }
    let found = sqlx::query_scalar::<_, String>(
        "SELECT id FROM organizations
         WHERE lower(regexp_replace(btrim(name), '\\s+', ' ', 'g')) = $1
         ORDER BY archived ASC, id ASC LIMIT 1",
    )
    .bind(normalize_property_part(name))
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;
    if let Some(id) = found {
        return Ok(id);
    }
    if !policy.create_missing_organizations {
        return Ok(String::new());
    }

    let input = OrganizationInput {
        name: name.to_string(),
        // Nothing in the file says what kind it is, and guessing would be worse
        // than leaving it for someone to set.
        kind: OrganizationKind::Other,
        ..Default::default()
    }
    .validate()?;
    let id = ids::opaque("org");
    sqlx::query(
        "INSERT INTO organizations (id, name, kind, website, phone, email, address, description)
         VALUES ($1, $2, $3, '', '', '', '', '')",
    )
    .bind(&id)
    .bind(&input.name)
    .bind(input.kind.slug())
    .execute(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;
    audit::record_in_transaction(
        tx,
        audit::Entity::Organization,
        &id,
        actor,
        "created",
        "",
        &input.name,
    )
    .await
    .map_err(|e| e.to_string())?;
    crate::server::db::organization_properties::add_defaults_for_new_organization(tx, &id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(id)
}

async fn write_contact(
    tx: &mut Transaction<'_, Postgres>,
    mapping: &Mapping,
    policy: &ImportPolicy,
    row: &[String],
    existing: Option<String>,
    actor: &str,
) -> Result<RowOutcome, String> {
    let organization_id = resolve_organization(
        tx,
        mapping.core_value(row, CoreField::OrganizationName),
        policy,
        actor,
    )
    .await?;

    match existing {
        None => {
            let input = ContactInput {
                first_name: mapping.core_value(row, CoreField::FirstName).to_string(),
                last_name: mapping.core_value(row, CoreField::LastName).to_string(),
                preferred_name: mapping.core_value(row, CoreField::PreferredName).to_string(),
                email: mapping.core_value(row, CoreField::Email).to_string(),
                phone: mapping.core_value(row, CoreField::Phone).to_string(),
                mobile: mapping.core_value(row, CoreField::Mobile).to_string(),
                address: mapping.core_value(row, CoreField::Address).to_string(),
                job_title: mapping.core_value(row, CoreField::JobTitle).to_string(),
                organization_id: organization_id.clone(),
                types: parse_types(
                    mapping.core_value(row, CoreField::ContactTypes),
                    policy.default_contact_type,
                ),
                source: mapping.core_value(row, CoreField::Source).to_string(),
                description: mapping.core_value(row, CoreField::Description).to_string(),
                do_not_contact: parse_bool(mapping.core_value(row, CoreField::DoNotContact)),
            }
            .validate()?;

            let id = ids::opaque("ct");
            sqlx::query(
                "INSERT INTO contacts
                    (id, first_name, last_name, preferred_name, email, phone, mobile, address,
                     job_title, organization_id, types, source, description, do_not_contact)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, nullif($10, ''), $11, $12, $13, $14)",
            )
            .bind(&id)
            .bind(&input.first_name)
            .bind(&input.last_name)
            .bind(&input.preferred_name)
            .bind(&input.email)
            .bind(&input.phone)
            .bind(&input.mobile)
            .bind(&input.address)
            .bind(&input.job_title)
            .bind(&input.organization_id)
            .bind(
                input
                    .types
                    .iter()
                    .map(|kind| kind.slug().to_string())
                    .collect::<Vec<_>>(),
            )
            .bind(&input.source)
            .bind(&input.description)
            .bind(input.do_not_contact)
            .execute(&mut **tx)
            .await
            .map_err(|e| e.to_string())?;

            audit::record_in_transaction(
                tx,
                audit::Entity::Contact,
                &id,
                actor,
                "created",
                "",
                "imported from a file",
            )
            .await
            .map_err(|e| e.to_string())?;

            // The standard blank fields first, exactly as `contacts::create`
            // gives them to a typed-in contact, so an imported record looks like
            // every other one. Merging afterwards fills them in rather than
            // appending a second copy.
            crate::server::db::contact_properties::add_defaults_for_new_contact(tx, &id)
                .await
                .map_err(|e| e.to_string())?;
            merge_properties(tx, PropertySubject::Contact, &id, mapping, row, actor).await?;
            Ok(RowOutcome::Created(id))
        }
        Some(id) => {
            // Only the columns the file actually maps are touched: an update
            // fills gaps, it does not overwrite the record with blanks.
            update_contact_fields(tx, &id, mapping, row, &organization_id).await?;
            audit::record_in_transaction(
                tx,
                audit::Entity::Contact,
                &id,
                actor,
                "updated",
                "",
                "imported from a file",
            )
            .await
            .map_err(|e| e.to_string())?;
            merge_properties(tx, PropertySubject::Contact, &id, mapping, row, actor).await?;
            Ok(RowOutcome::Updated(id))
        }
    }
}

/// Update just the mapped, non-blank core columns of an existing contact.
async fn update_contact_fields(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    mapping: &Mapping,
    row: &[String],
    organization_id: &str,
) -> Result<(), String> {
    use crate::server_fns::crm::{clean_text, MAX_LONG_TEXT, MAX_NAME, MAX_SHORT_TEXT};

    // (column, value) pairs, built from the mapping so an unmapped field is
    // never named in the UPDATE at all.
    let candidates: [(&str, CoreField, usize); 8] = [
        ("first_name", CoreField::FirstName, MAX_NAME),
        ("last_name", CoreField::LastName, MAX_NAME),
        ("preferred_name", CoreField::PreferredName, MAX_NAME),
        ("phone", CoreField::Phone, MAX_SHORT_TEXT),
        ("mobile", CoreField::Mobile, MAX_SHORT_TEXT),
        ("address", CoreField::Address, MAX_SHORT_TEXT),
        ("job_title", CoreField::JobTitle, MAX_NAME),
        ("source", CoreField::Source, MAX_SHORT_TEXT),
    ];

    for (column, field, max) in candidates {
        let value = mapping.core_value(row, field);
        if value.trim().is_empty() {
            continue;
        }
        let value = clean_text(value, &field.label().to_lowercase(), max)?;
        // The column name is a compile-time literal from the table above; only
        // the value is ever bound from the file.
        sqlx::query(&format!("UPDATE contacts SET {column} = $2 WHERE id = $1"))
            .bind(id)
            .bind(&value)
            .execute(&mut **tx)
            .await
            .map_err(|e| e.to_string())?;
    }

    let description = mapping.core_value(row, CoreField::Description);
    if !description.trim().is_empty() {
        let value = clean_text(description, "description", MAX_LONG_TEXT)?;
        sqlx::query("UPDATE contacts SET description = $2 WHERE id = $1")
            .bind(id)
            .bind(&value)
            .execute(&mut **tx)
            .await
            .map_err(|e| e.to_string())?;
    }

    // Raw, with no fallback: a file that says nothing about types must leave
    // the contact's existing ones alone.
    let types = parse_types_raw(mapping.core_value(row, CoreField::ContactTypes));
    if !types.is_empty() {
        sqlx::query("UPDATE contacts SET types = $2 WHERE id = $1")
            .bind(id)
            .bind(
                types
                    .iter()
                    .map(|kind| kind.slug().to_string())
                    .collect::<Vec<_>>(),
            )
            .execute(&mut **tx)
            .await
            .map_err(|e| e.to_string())?;
    }

    // Only ever set to true: a file that omits the column must not silently
    // re-enable contact for someone who asked to be left alone.
    if parse_bool(mapping.core_value(row, CoreField::DoNotContact)) {
        sqlx::query("UPDATE contacts SET do_not_contact = TRUE WHERE id = $1")
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(|e| e.to_string())?;
    }

    if !organization_id.is_empty() {
        sqlx::query("UPDATE contacts SET organization_id = $2 WHERE id = $1")
            .bind(id)
            .bind(organization_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn write_organization(
    tx: &mut Transaction<'_, Postgres>,
    mapping: &Mapping,
    row: &[String],
    existing: Option<String>,
    actor: &str,
) -> Result<RowOutcome, String> {
    match existing {
        None => {
            let input = OrganizationInput {
                name: mapping.core_value(row, CoreField::OrgName).to_string(),
                kind: parse_kind(mapping.core_value(row, CoreField::OrgKind)),
                website: mapping.core_value(row, CoreField::OrgWebsite).to_string(),
                phone: mapping.core_value(row, CoreField::OrgPhone).to_string(),
                email: mapping.core_value(row, CoreField::OrgEmail).to_string(),
                address: mapping.core_value(row, CoreField::OrgAddress).to_string(),
                description: mapping.core_value(row, CoreField::OrgDescription).to_string(),
            }
            .validate()?;

            let id = ids::opaque("org");
            sqlx::query(
                "INSERT INTO organizations
                    (id, name, kind, website, phone, email, address, description)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(&id)
            .bind(&input.name)
            .bind(input.kind.slug())
            .bind(&input.website)
            .bind(&input.phone)
            .bind(&input.email)
            .bind(&input.address)
            .bind(&input.description)
            .execute(&mut **tx)
            .await
            .map_err(|e| e.to_string())?;

            audit::record_in_transaction(
                tx,
                audit::Entity::Organization,
                &id,
                actor,
                "created",
                "",
                "imported from a file",
            )
            .await
            .map_err(|e| e.to_string())?;

            crate::server::db::organization_properties::add_defaults_for_new_organization(tx, &id)
                .await
                .map_err(|e| e.to_string())?;
            merge_properties(tx, PropertySubject::Organization, &id, mapping, row, actor).await?;
            Ok(RowOutcome::Created(id))
        }
        Some(id) => {
            update_organization_fields(tx, &id, mapping, row).await?;
            audit::record_in_transaction(
                tx,
                audit::Entity::Organization,
                &id,
                actor,
                "updated",
                "",
                "imported from a file",
            )
            .await
            .map_err(|e| e.to_string())?;
            merge_properties(tx, PropertySubject::Organization, &id, mapping, row, actor).await?;
            Ok(RowOutcome::Updated(id))
        }
    }
}

async fn update_organization_fields(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    mapping: &Mapping,
    row: &[String],
) -> Result<(), String> {
    use crate::server_fns::crm::{clean_text, MAX_LONG_TEXT, MAX_SHORT_TEXT};

    let candidates: [(&str, CoreField, usize); 5] = [
        ("website", CoreField::OrgWebsite, MAX_SHORT_TEXT),
        ("phone", CoreField::OrgPhone, MAX_SHORT_TEXT),
        ("email", CoreField::OrgEmail, MAX_SHORT_TEXT),
        ("address", CoreField::OrgAddress, MAX_SHORT_TEXT),
        ("description", CoreField::OrgDescription, MAX_LONG_TEXT),
    ];
    for (column, field, max) in candidates {
        let value = mapping.core_value(row, field);
        if value.trim().is_empty() {
            continue;
        }
        let value = clean_text(value, &field.label().to_lowercase(), max)?;
        sqlx::query(&format!(
            "UPDATE organizations SET {column} = $2 WHERE id = $1"
        ))
        .bind(id)
        .bind(&value)
        .execute(&mut **tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    let kind_cell = mapping.core_value(row, CoreField::OrgKind);
    if !kind_cell.trim().is_empty() {
        sqlx::query("UPDATE organizations SET kind = $2 WHERE id = $1")
            .bind(id)
            .bind(parse_kind(kind_cell).slug())
            .execute(&mut **tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Merge this row's property columns into the record's list.
///
/// Matches [`crate::server::db::bulk_properties::apply`]: an existing row keeps
/// its stored spelling and its `ord` and only its value changes; a new one is
/// appended at the end. A record already holding the maximum number of
/// properties simply stops gaining them rather than failing the row.
async fn merge_properties(
    tx: &mut Transaction<'_, Postgres>,
    subject: PropertySubject,
    record_id: &str,
    mapping: &Mapping,
    row: &[String],
    _actor: &str,
) -> Result<(), String> {
    use crate::server_fns::contact_properties::MAX_VALUE_CHARS;

    let values = mapping.property_values(row);
    if values.is_empty() {
        return Ok(());
    }

    let (table, owner) = match subject {
        PropertySubject::Contact => ("contact_properties", "contact_id"),
        PropertySubject::Organization => ("organization_properties", "organization_id"),
    };

    // The record's current list, read inside the transaction so a concurrent
    // edit cannot make the decision stale.
    let existing = sqlx::query_as::<_, (i32, String, String)>(&format!(
        "SELECT ord, section, key FROM {table} WHERE {owner} = $1 ORDER BY ord"
    ))
    .bind(record_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;

    let mut by_name: HashMap<(String, String), i32> = HashMap::new();
    for (ord, section, key) in &existing {
        by_name
            .entry((
                normalize_property_part(section),
                normalize_property_part(key),
            ))
            .or_insert(*ord);
    }
    let mut next_ord = existing.iter().map(|(ord, _, _)| *ord).max().unwrap_or(-1) + 1;
    let mut count = existing.len();

    for (section, key, value) in values {
        let value: String = value.chars().take(MAX_VALUE_CHARS).collect();
        let normalized = (
            normalize_property_part(&section),
            normalize_property_part(&key),
        );
        match by_name.get(&normalized) {
            Some(ord) => {
                sqlx::query(&format!(
                    "UPDATE {table} SET value = $3 WHERE {owner} = $1 AND ord = $2"
                ))
                .bind(record_id)
                .bind(ord)
                .bind(&value)
                .execute(&mut **tx)
                .await
                .map_err(|e| e.to_string())?;
            }
            None => {
                if count >= MAX_PROPERTIES {
                    continue;
                }
                sqlx::query(&format!(
                    "INSERT INTO {table} ({owner}, ord, key, value, section)
                     VALUES ($1, $2, $3, $4, $5)"
                ))
                .bind(record_id)
                .bind(next_ord)
                .bind(&key)
                .bind(&value)
                .bind(&section)
                .execute(&mut **tx)
                .await
                .map_err(|e| e.to_string())?;
                by_name.insert(normalized, next_ord);
                next_ord += 1;
                count += 1;
            }
        }
    }
    Ok(())
}
