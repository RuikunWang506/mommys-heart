-- Contact status, richer outreach history, and outreach logged from mail tasks.
--
-- 1. A contact gains a relationship status. `do_not_contact` stays as the flag
--    the mail tool already honors; existing do-not-contact rows start with the
--    matching status so the two agree from day one.
-- 2. An outreach entry can record who owns it, the response, and the next step.
-- 3. An entry can point at the mail task that produced it, so a group email is
--    logged once per recipient and traceable back to the send.

ALTER TABLE contacts
    ADD COLUMN status TEXT NOT NULL DEFAULT 'active',
    ADD CONSTRAINT contacts_status_check CHECK (status IN (
        'active', 'prospective', 'outreach_initiated', 'awaiting_response',
        'follow_up_needed', 'engaged', 'partner', 'inactive', 'former',
        'do_not_contact'
    ));

UPDATE contacts SET status = 'do_not_contact' WHERE do_not_contact;

CREATE INDEX contacts_status_idx ON contacts (status) WHERE NOT archived;

ALTER TABLE contact_communications
    ADD COLUMN assigned_to_id   TEXT REFERENCES users(id) ON DELETE SET NULL,
    ADD COLUMN assigned_to_name TEXT NOT NULL DEFAULT '',
    ADD COLUMN response         TEXT NOT NULL DEFAULT '',
    ADD COLUMN next_step        TEXT NOT NULL DEFAULT '',
    ADD COLUMN next_step_date   DATE,
    ADD COLUMN mail_task_id     TEXT REFERENCES contact_mail_tasks(id) ON DELETE SET NULL;

-- One logged entry per contact per mail task, so finishing a task twice cannot
-- double-log it.
CREATE UNIQUE INDEX contact_communications_mail_task_idx
    ON contact_communications (mail_task_id, contact_id)
    WHERE mail_task_id IS NOT NULL;

-- Open follow-ups are the list staff work from.
CREATE INDEX contact_communications_next_step_idx
    ON contact_communications (next_step_date)
    WHERE next_step_date IS NOT NULL;
