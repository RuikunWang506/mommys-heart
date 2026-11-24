//! The signed Service Agreement a client accepted at signup: their electronic
//! signature, contact and emergency contact, and the services they asked for.
//!
//! The client twin of the signature columns on [`super::volunteers`]. Written
//! once, inside the transaction that creates the account and case, so the
//! signature and what it was given for commit together or not at all.

use crate::helpers::client_details::ClientAgreementDetails;

/// The name the Service Agreement for `case_id` was electronically signed with,
/// if one was recorded. Shown beside the acceptance date on the case.
pub async fn signer_for_case(case_id: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT signature_name FROM client_agreements
         WHERE case_id = $1 AND signature_name <> ''
         LIMIT 1",
    )
    .bind(case_id)
    .fetch_optional(crate::server::db::pool())
    .await
}

/// Signup details used to prefill the general intake.
#[derive(sqlx::FromRow)]
pub struct IntakePrefill {
    pub first_name: String,
    pub last_name: String,
    pub date_of_birth: String,
    pub phone: String,
    pub email: String,
}

pub async fn intake_prefill_for_case(
    case_id: &str,
) -> Result<Option<IntakePrefill>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.first_name, u.last_name, COALESCE(to_char(a.date_of_birth, 'YYYY-MM-DD'), '') AS date_of_birth,
                a.phone, u.email
         FROM client_agreements a JOIN users u ON u.id = a.user_id
         WHERE a.case_id = $1
         LIMIT 1",
    )
    .bind(case_id)
    .fetch_optional(crate::server::db::pool())
    .await
}

/// Record a signed agreement inside the caller's transaction.
pub async fn insert_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: &str,
    case_id: Option<&str>,
    terms_version: &str,
    details: &ClientAgreementDetails,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO client_agreements
             (user_id, case_id, terms_version, legal_name, signature_name, email, date_of_birth,
              phone, emergency_name, emergency_relationship, emergency_phone, services_requested,
              electronic_consent, minor_name, guardian_name, guardian_relationship,
              guardian_signature, guardian_email)
         VALUES ($1, $2, $3, $4, $5, $6, NULLIF($7, '')::date, $8, $9, $10, $11, $12, $13, $14,
                 $15, $16, $17, $18)
         ON CONFLICT (user_id) DO NOTHING",
    )
    .bind(user_id)
    .bind(case_id)
    .bind(terms_version)
    .bind(&details.legal_name)
    .bind(&details.signature_name)
    .bind(&details.email)
    .bind(&details.date_of_birth)
    .bind(&details.phone)
    .bind(&details.emergency_name)
    .bind(&details.emergency_relationship)
    .bind(&details.emergency_phone)
    .bind(details.services_line())
    .bind(details.electronic_consent)
    .bind(&details.minor_name)
    .bind(&details.guardian_name)
    .bind(&details.guardian_relationship)
    .bind(&details.guardian_signature)
    .bind(&details.guardian_email)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
