ALTER TABLE agent_evidence_upload_grants
    DROP CONSTRAINT agent_evidence_upload_grants_completion,
    ADD CONSTRAINT agent_evidence_upload_grants_completion
        CHECK (
            (completed_at IS NULL AND document_id IS NULL)
            OR
            (
                completed_at IS NOT NULL
                AND document_id IS NOT NULL
                AND completed_at >= issued_at
                AND completed_at < expires_at
            )
        );

ALTER TABLE agent_policy_document_upload_grants
    DROP CONSTRAINT agent_policy_document_upload_grants_completion,
    ADD CONSTRAINT agent_policy_document_upload_grants_completion
        CHECK (
            (completed_at IS NULL AND document_id IS NULL)
            OR
            (
                completed_at IS NOT NULL
                AND document_id IS NOT NULL
                AND completed_at >= issued_at
                AND completed_at < expires_at
            )
        );
