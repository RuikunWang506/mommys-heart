-- The approved case plan: actions staff have agreed on, and the tasks assigned
-- for them. A task stays active through any number of attempts until a final
-- outcome is recorded.
CREATE TABLE case_plan_actions (
    id            TEXT PRIMARY KEY,
    case_id       TEXT NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
    title         TEXT NOT NULL,
    added_by_id   TEXT REFERENCES users(id) ON DELETE SET NULL,
    added_by_name TEXT NOT NULL,
    added_on      DATE NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    seq           BIGSERIAL,
    CONSTRAINT case_plan_actions_title_check CHECK (btrim(title) <> '')
);

CREATE INDEX case_plan_actions_case_id_idx ON case_plan_actions (case_id, seq);

CREATE TABLE case_tasks (
    id                  TEXT PRIMARY KEY,
    action_id           TEXT NOT NULL REFERENCES case_plan_actions(id) ON DELETE CASCADE,
    assignee_id         TEXT REFERENCES users(id) ON DELETE SET NULL,
    assignee_name       TEXT NOT NULL,
    follow_up_on        DATE,
    outcome             TEXT NOT NULL DEFAULT '',
    outcome_notes       TEXT NOT NULL DEFAULT '',
    outcome_recorded_by TEXT NOT NULL DEFAULT '',
    outcome_recorded_on DATE,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    seq                 BIGSERIAL,
    CONSTRAINT case_tasks_outcome_check CHECK (outcome IN (
        '', 'completed', 'declined_by_client', 'no_longer_needed'
    ))
);

CREATE INDEX case_tasks_action_id_idx ON case_tasks (action_id, seq);
CREATE INDEX case_tasks_assignee_id_idx ON case_tasks (assignee_id) WHERE outcome = '';

CREATE TABLE case_task_attempts (
    id               TEXT PRIMARY KEY,
    task_id          TEXT NOT NULL REFERENCES case_tasks(id) ON DELETE CASCADE,
    result           TEXT NOT NULL,
    note             TEXT NOT NULL DEFAULT '',
    next_try         DATE NOT NULL,
    recorded_by_id   TEXT REFERENCES users(id) ON DELETE SET NULL,
    recorded_by_name TEXT NOT NULL,
    recorded_on      DATE NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    seq              BIGSERIAL,
    CONSTRAINT case_task_attempts_result_check CHECK (result IN (
        'could_not_reach', 'left_message', 'talked_not_done', 'waiting_on_others'
    ))
);

CREATE INDEX case_task_attempts_task_id_idx ON case_task_attempts (task_id, seq);
