//! A case's team: the users holding `case_assignments` on one case, read and
//! edited from the case's side. The rows are the same ones the user card edits.

use std::fmt;

use crate::server::db::{audit, pool, users};
use crate::server_fns::capabilities::CaseCapability;
use crate::server_fns::case_team::CaseTeamMember;
use crate::server_fns::users::{AccountRole, UserSummary};

#[derive(Debug)]
pub enum Error {
    Database(sqlx::Error),
    NotFound(String),
    Invalid(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "{error}"),
            Self::NotFound(message) | Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// Everyone assigned to the case, with their capabilities, ordered by name.
pub async fn members(case_id: &str) -> Result<Vec<CaseTeamMember>, sqlx::Error> {
    let rows: Vec<(String, String, String, String, String, Vec<String>)> = sqlx::query_as(
        "SELECT u.id, u.first_name, u.last_name, u.email, u.role,
                array_agg(a.capability ORDER BY a.capability)
         FROM case_assignments a
         JOIN users u ON u.id = a.user_id
         WHERE a.case_id = $1
         GROUP BY u.id, u.first_name, u.last_name, u.email, u.role
         ORDER BY lower(u.first_name), lower(u.last_name), u.id",
    )
    .bind(case_id)
    .fetch_all(pool())
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(user_id, first_name, last_name, email, role, slugs)| CaseTeamMember {
                user_id,
                first_name,
                last_name,
                email,
                role: AccountRole::from_slug(&role).unwrap_or(AccountRole::Client),
                capabilities: slugs
                    .iter()
                    .filter_map(|slug| CaseCapability::from_slug(slug))
                    .collect(),
            },
        )
        .collect())
}

/// Volunteers and operations admins matching `search`, the people who can be
/// added to a case team. Site admins already see every case, so they are left out.
pub async fn search_assignable(search: &str, limit: i64) -> Result<Vec<UserSummary>, sqlx::Error> {
    let limit = limit.clamp(1, 500);
    let term = search.trim();
    let pattern = (!term.is_empty()).then(|| {
        format!(
            "%{}%",
            term.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        )
    });
    let rows: Vec<(String, String, String, String, bool)> = sqlx::query_as(
        "SELECT id, first_name, last_name, role, information_management_access
         FROM users
         WHERE role IN ('volunteer', 'operations_admin')
           AND ($1::text IS NULL
                OR first_name ILIKE $1
                OR last_name ILIKE $1
                OR (first_name || ' ' || last_name) ILIKE $1
                OR email ILIKE $1)
         ORDER BY lower(first_name), lower(last_name), id
         LIMIT $2",
    )
    .bind(&pattern)
    .bind(limit)
    .fetch_all(pool())
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(id, first_name, last_name, role, information_management_access)| UserSummary {
                id,
                first_name,
                last_name,
                role: AccountRole::from_slug(&role).unwrap_or(AccountRole::Volunteer),
                information_management_access,
            },
        )
        .collect())
}

/// Apply several users' changes on one case in a single transaction: `Some`
/// sets that user's capabilities, `None` removes them. Returns the users who
/// were newly given `ViewCase`, so the caller can notify them.
pub async fn apply(
    case_id: &str,
    changes: &[(String, Option<Vec<CaseCapability>>)],
    actor: &str,
) -> Result<Vec<String>, Error> {
    let mut tx = pool().begin().await?;
    let case_exists: Option<i32> = sqlx::query_scalar("SELECT 1 FROM cases WHERE id = $1 FOR SHARE")
        .bind(case_id)
        .fetch_optional(&mut *tx)
        .await?;
    if case_exists.is_none() {
        return Err(Error::NotFound("Case not found.".to_string()));
    }

    let mut newly_assigned = Vec::new();
    for (user_id, capabilities) in changes {
        let row: Option<(String, String, String)> =
            sqlx::query_as("SELECT role, first_name, last_name FROM users WHERE id = $1 FOR UPDATE")
                .bind(user_id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some((role, first_name, last_name)) = row else {
            return Err(Error::NotFound("One of the selected people no longer exists.".to_string()));
        };
        let name = format!("{first_name} {last_name}").trim().to_string();
        match AccountRole::from_slug(&role) {
            Some(AccountRole::Deactivated) => {
                return Err(Error::Invalid(format!(
                    "{name}'s account is deactivated. Reactivate it first."
                )));
            }
            Some(AccountRole::SiteAdmin) if capabilities.is_some() => {
                return Err(Error::Invalid(format!(
                    "{name} is a site admin and already has full access to every case."
                )));
            }
            _ => {}
        }

        let was_assigned: Option<i32> = sqlx::query_scalar(
            "SELECT 1 FROM case_assignments WHERE user_id = $1 AND case_id = $2 LIMIT 1",
        )
        .bind(user_id)
        .bind(case_id)
        .fetch_optional(&mut *tx)
        .await?;

        match capabilities {
            Some(capabilities) => {
                users::assign_capabilities_in(&mut tx, user_id, case_id, capabilities, actor)
                    .await?;
                if was_assigned.is_none() && capabilities.contains(&CaseCapability::ViewCase) {
                    newly_assigned.push(user_id.clone());
                }
            }
            None => {
                sqlx::query("DELETE FROM case_assignments WHERE user_id = $1 AND case_id = $2")
                    .bind(user_id)
                    .bind(case_id)
                    .execute(&mut *tx)
                    .await?;
                audit::record_in_transaction(
                    &mut tx,
                    audit::Entity::User,
                    user_id,
                    actor,
                    &format!("case:{case_id}"),
                    "assigned",
                    "removed",
                )
                .await?;
            }
        }
    }
    tx.commit().await?;
    Ok(newly_assigned)
}
