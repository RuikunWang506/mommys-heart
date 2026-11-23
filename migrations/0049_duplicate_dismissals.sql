-- Groups of records someone has checked on the "Possible duplicates" list and
-- marked as not duplicates (for example, two different clients who share a
-- name). A group is identified by its exact set of record ids, sorted, so if
-- another matching record appears later the group is shown again for review.
CREATE TABLE duplicate_dismissals (
    kind              TEXT NOT NULL,
    record_ids        TEXT[] NOT NULL,
    dismissed_by_id   TEXT REFERENCES users(id) ON DELETE SET NULL,
    dismissed_by_name TEXT NOT NULL,
    dismissed_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (kind, record_ids),
    CONSTRAINT duplicate_dismissals_kind_check CHECK (kind IN ('case', 'contact')),
    CONSTRAINT duplicate_dismissals_ids_check CHECK (cardinality(record_ids) >= 2)
);
