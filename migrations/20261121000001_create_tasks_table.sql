CREATE TABLE tasks (
    id          UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    title       VARCHAR(200) NOT NULL,
    description TEXT         NULL,
    status      TEXT         NOT NULL DEFAULT 'todo',
    priority    TEXT         NOT NULL DEFAULT 'medium',
    due_date    DATE         NULL,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT now(),
    CONSTRAINT tasks_title_check       CHECK (char_length(btrim(title)) BETWEEN 1 AND 200),
    CONSTRAINT tasks_description_check CHECK (description IS NULL OR char_length(description) <= 5000),
    CONSTRAINT tasks_status_check      CHECK (status IN ('todo', 'in_progress', 'done')),
    CONSTRAINT tasks_priority_check    CHECK (priority IN ('low', 'medium', 'high'))
);

CREATE INDEX idx_tasks_status     ON tasks (status);
CREATE INDEX idx_tasks_priority   ON tasks (priority);
CREATE INDEX idx_tasks_due_date   ON tasks (due_date);
CREATE INDEX idx_tasks_created_at ON tasks (created_at);
