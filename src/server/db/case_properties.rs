//! Case property persistence (SSR only): the `case_properties` table.
//!
//! Kept separate from [`crate::server::db::cases`] because properties are their
//! own list with their own ordering, visibility, and replace-in-place write,
//! none of which the case row itself is involved in.

use std::collections::HashMap;

use crate::helpers::new_case_fields;
use crate::helpers::visibility::Visibility;
use crate::server::db::{audit, pool};
use crate::server_fns::case_properties::CaseProperty;

/// The properties on a case that the caller may see, in display order.
///
/// `include_volunteer_only` comes from the caller's account role, and the filter
/// is applied in SQL, so a client's response never contains the volunteer-only
/// properties in the first place.
pub async fn get_case_properties(
    case_id: &str,
    include_volunteer_only: bool,
) -> Result<Vec<CaseProperty>, sqlx::Error> {
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT key, value, section, visibility FROM case_properties
         WHERE case_id = $1 AND ($2 OR visibility <> $3)
         ORDER BY visibility ASC, ord ASC",
    )
    .bind(case_id)
    .bind(include_volunteer_only)
    .bind(Visibility::VolunteerOnly.slug())
    .fetch_all(pool())
    .await?;

    Ok(rows
        .into_iter()
        .map(|(key, value, section, visibility)| CaseProperty {
            key,
            value,
            section,
            visibility: Visibility::from_slug(&visibility).unwrap_or_default(),
        })
        .collect())
}

/// Tracks the next display position within each visibility. `ord` is part of the
/// primary key together with `visibility`, so each list counts from zero
/// independently and rewriting one never renumbers the other.
#[derive(Default)]
struct Ordering(HashMap<Visibility, i32>);

/// Insert one property, taking the next position in its visibility's list.
///
/// A blank `value` is allowed and meaningful: it is how a property appears as a
/// labelled blank waiting to be filled in.
async fn insert(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    case_id: &str,
    ordering: &mut Ordering,
    property: &CaseProperty,
) -> Result<(), sqlx::Error> {
    let next = ordering.0.entry(property.visibility).or_insert(0);
    sqlx::query(
        "INSERT INTO case_properties (case_id, ord, key, value, section, visibility)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(case_id)
    .bind(*next)
    .bind(&property.key)
    .bind(&property.value)
    .bind(&property.section)
    .bind(property.visibility.slug())
    .execute(&mut **tx)
    .await?;
    *next += 1;
    Ok(())
}

/// The properties a new case starts with, from
/// [`VOLUNTEER_ONLY_FIELDS`](crate::helpers::new_case_fields::VOLUNTEER_ONLY_FIELDS):
/// each one named but not yet filled in.
fn for_new_case() -> Vec<CaseProperty> {
    new_case_fields::volunteer_only_properties()
        .map(|f| CaseProperty {
            key: f.label.to_string(),
            value: String::new(),
            section: f.section.to_string(),
            visibility: Visibility::VolunteerOnly,
        })
        .collect()
}

/// Insert the standing new-case paperwork from
/// [`VOLUNTEER_ONLY_FIELDS`](crate::helpers::new_case_fields::VOLUNTEER_ONLY_FIELDS)
/// first, then `extra`, numbered as one list per visibility, inside the caller's
/// transaction so the whole starting set lands with the case or not at all. The
/// standing paperwork comes first so it sits above whatever the creator typed in.
pub async fn add_for_new_case(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    case_id: &str,
    extra: impl IntoIterator<Item = CaseProperty>,
) -> Result<(), sqlx::Error> {
    let mut starting = for_new_case();
    starting.extend(extra);
    let mut ordering = Ordering::default();
    for property in &starting {
        insert(tx, case_id, &mut ordering, property).await?;
    }
    Ok(())
}

/// Trim a submitted property list, dropping the entries that cannot be stored.
///
/// A blank value is kept — that is a property nobody has filled in yet. A blank
/// key is dropped, because an unlabelled property has nothing to show.
pub fn clean(properties: Vec<CaseProperty>) -> Vec<CaseProperty> {
    properties
        .into_iter()
        .filter_map(|p| {
            let key = p.key.trim().to_string();
            if key.is_empty() {
                return None;
            }
            Some(CaseProperty {
                key,
                value: p.value.trim().to_string(),
                section: p.section.trim().to_string(),
                visibility: p.visibility,
            })
        })
        .collect()
}

/// Replace the properties a case holds at one visibility, auditing once when
/// they actually change.
///
/// Deliberately scoped to a single visibility. The write is a delete followed by
/// a reinsert, and both visibilities share this table; an unscoped delete would
/// let someone editing the shared list — which a client can do, and whose form
/// never even loads the volunteer-only properties — destroy the team's intake
/// record.
pub async fn replace(
    case_id: &str,
    visibility: Visibility,
    properties: Vec<CaseProperty>,
    actor: &str,
) -> Result<(), sqlx::Error> {
    use crate::helpers::case_questionnaires::is_questionnaire_section;

    // The submitted visibility wins over whatever each row claims: this call
    // replaces exactly one list, so every row in it belongs to that list.
    //
    // Questionnaire sections are not this call's to write. They belong to
    // `replace_section`, which validates the answers first, so rows the caller
    // submits for them are dropped and the stored ones carried through
    // untouched. Same reasoning as scoping this to one visibility: a write made
    // from one list must not clobber a list it never loaded.
    let cleaned: Vec<CaseProperty> = clean(properties)
        .into_iter()
        .filter(|p| !is_questionnaire_section(&p.section))
        .map(|p| CaseProperty { visibility, ..p })
        .collect();

    let existing = get_at(case_id, visibility).await?;
    let mut replacement = cleaned;
    replacement.extend(
        existing
            .iter()
            .filter(|p| is_questionnaire_section(&p.section))
            .cloned(),
    );
    let changed = existing != replacement;

    let mut tx = pool().begin().await?;
    sqlx::query("DELETE FROM case_properties WHERE case_id = $1 AND visibility = $2")
        .bind(case_id)
        .bind(visibility.slug())
        .execute(&mut *tx)
        .await?;
    let mut ordering = Ordering::default();
    for property in &replacement {
        insert(&mut tx, case_id, &mut ordering, property).await?;
    }
    tx.commit().await?;

    if changed {
        audit::record(
            pool(),
            audit::Entity::Case,
            case_id,
            actor,
            "properties",
            "",
            "updated",
        )
        .await?;
    }
    Ok(())
}

/// The properties a case holds at one visibility, in display order.
async fn get_at(case_id: &str, visibility: Visibility) -> Result<Vec<CaseProperty>, sqlx::Error> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT key, value, section FROM case_properties
         WHERE case_id = $1 AND visibility = $2 ORDER BY ord ASC",
    )
    .bind(case_id)
    .bind(visibility.slug())
    .fetch_all(pool())
    .await?;

    Ok(rows
        .into_iter()
        .map(|(key, value, section)| CaseProperty {
            key,
            value,
            section,
            visibility,
        })
        .collect())
}

pub enum SectionWrite<'a> {
    Submit { audit_event: &'a str },
    Draft,
}

const DRAFT_AUDIT_EVENT: &str = "questionnaire draft saved";

/// Replace one section while preserving every other property at this
/// visibility. The complete list is rewritten to keep display ordering stable.
pub async fn replace_section(
    case_id: &str,
    visibility: Visibility,
    section: &str,
    properties: Vec<CaseProperty>,
    actor: &str,
    write: SectionWrite<'_>,
) -> Result<bool, sqlx::Error> {
    let cleaned = clean(properties)
        .into_iter()
        .map(|property| CaseProperty {
            section: section.to_string(),
            visibility,
            ..property
        })
        .collect::<Vec<_>>();
    let mut tx = pool().begin().await?;
    sqlx::query("SELECT id FROM cases WHERE id = $1 FOR UPDATE")
        .bind(case_id)
        .execute(&mut *tx)
        .await?;
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT key, value, section FROM case_properties
         WHERE case_id = $1 AND visibility = $2 ORDER BY ord ASC",
    )
    .bind(case_id)
    .bind(visibility.slug())
    .fetch_all(&mut *tx)
    .await?;
    let existing = rows
        .into_iter()
        .map(|(key, value, section)| CaseProperty {
            key,
            value,
            section,
            visibility,
        })
        .collect::<Vec<_>>();
    let current = existing
        .iter()
        .filter(|property| property.section == section)
        .cloned()
        .collect::<Vec<_>>();
    if matches!(write, SectionWrite::Draft)
        && current
            .iter()
            .any(|property| property.key == crate::helpers::case_questionnaires::COMPLETED_KEY)
    {
        tx.commit().await?;
        return Ok(false);
    }
    if current == cleaned {
        tx.commit().await?;
        return Ok(true);
    }

    let mut replacement = Vec::with_capacity(existing.len() - current.len() + cleaned.len());
    let mut inserted = false;
    for property in existing {
        if property.section == section {
            if !inserted {
                replacement.extend(cleaned.iter().cloned());
                inserted = true;
            }
        } else {
            replacement.push(property);
        }
    }
    if !inserted {
        replacement.extend(cleaned);
    }

    sqlx::query("DELETE FROM case_properties WHERE case_id = $1 AND visibility = $2")
        .bind(case_id)
        .bind(visibility.slug())
        .execute(&mut *tx)
        .await?;
    let mut ordering = Ordering::default();
    for property in &replacement {
        insert(&mut tx, case_id, &mut ordering, property).await?;
    }
    tx.commit().await?;

    match write {
        SectionWrite::Submit { audit_event } => {
            audit::record(pool(), audit::Entity::Case, case_id, actor, section, "", audit_event)
                .await?;
        }
        SectionWrite::Draft => {
            let recently_logged: bool = sqlx::query_scalar(
                "SELECT EXISTS (
                     SELECT 1 FROM audit_log
                     WHERE entity_type = 'case' AND entity_id = $1 AND field = $2 AND actor = $3
                       AND new_value = $4 AND created_at > now() - interval '30 minutes'
                 )",
            )
            .bind(case_id)
            .bind(section)
            .bind(actor)
            .bind(DRAFT_AUDIT_EVENT)
            .fetch_one(pool())
            .await?;
            if !recently_logged {
                audit::record_with_visibility(
                    pool(),
                    audit::Entity::Case,
                    case_id,
                    actor,
                    section,
                    "",
                    DRAFT_AUDIT_EVENT,
                    visibility,
                )
                .await?;
            }
        }
    }
    Ok(true)
}
