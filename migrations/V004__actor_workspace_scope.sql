-- Actors gain a real workspace home so tenant isolation lives in Postgres, not
-- only in an external authorization engine. Existing actors are backfilled to a
-- dedicated system workspace before the column is made NOT NULL.
ALTER TABLE actors ADD COLUMN workspace_id UUID REFERENCES workspaces(id);
ALTER TABLE actors ADD COLUMN created_by_user_id UUID REFERENCES users(id);

INSERT INTO workspaces (id, slug, name)
VALUES ('00000000-0000-4000-8000-0000000000ff', 'system', 'System')
ON CONFLICT (id) DO NOTHING;

UPDATE actors
SET workspace_id = '00000000-0000-4000-8000-0000000000ff'
WHERE workspace_id IS NULL;

ALTER TABLE actors ALTER COLUMN workspace_id SET NOT NULL;

-- Allow key rotation: more than one live credential per actor. Authentication
-- now resolves the credential by key_id scoped to the actor.
ALTER TABLE api_credentials DROP CONSTRAINT IF EXISTS api_credentials_actor_id_key;
DROP INDEX IF EXISTS idx_api_credentials_actor_id;
CREATE INDEX idx_api_credentials_actor_id ON api_credentials (actor_id);

-- Per-actor data-plane permission grants. Replaces the external authorization
-- engine: an actor may use a data API only if it holds the matching permission.
CREATE TABLE actor_permissions (
    actor_id   UUID NOT NULL REFERENCES actors(id) ON DELETE CASCADE,
    permission TEXT NOT NULL CHECK (permission IN (
        'read_evidence_requests', 'write_evidence_requests',
        'read_evidence_submissions', 'write_evidence_submissions',
        'read_controls', 'write_controls')),
    PRIMARY KEY (actor_id, permission)
);

-- Existing actors keep full data-plane access (every member previously held all
-- six permissions), so current keys keep working.
INSERT INTO actor_permissions (actor_id, permission)
SELECT actors.id, granted.permission
FROM actors
CROSS JOIN (VALUES
    ('read_evidence_requests'),
    ('write_evidence_requests'),
    ('read_evidence_submissions'),
    ('write_evidence_submissions'),
    ('read_controls'),
    ('write_controls')
) AS granted(permission);
