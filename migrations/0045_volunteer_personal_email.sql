-- The personal address a volunteer applied with, kept once they sign in with an
-- organizational one. Empty when they still sign in with the address they applied with.
ALTER TABLE volunteers ADD COLUMN personal_email TEXT NOT NULL DEFAULT '';

UPDATE volunteers v
SET personal_email = a.email
FROM volunteer_applicants a
JOIN users u ON u.id = a.completed_user_id
WHERE a.completed_user_id = v.user_id
  AND lower(a.email) <> lower(u.email);
