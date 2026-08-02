ALTER TABLE outbox_messages
    ADD COLUMN message_kind TEXT,
    ADD COLUMN message_type TEXT,
    ADD COLUMN message_version INTEGER,
    ADD COLUMN message_id UUID DEFAULT gen_random_uuid(),
    ADD COLUMN subject TEXT,
    ADD COLUMN correlation_id UUID,
    ADD COLUMN causation_id UUID;

UPDATE outbox_messages
SET
    message_kind = CASE
        WHEN event_type IN (
            'document.scan_requested',
            'document.finalization_requested'
        ) THEN 'command'
        ELSE 'event'
    END,
    message_type = event_type,
    message_version = 0,
    subject = aggregate_id,
    correlation_id = request_id;

ALTER TABLE outbox_messages
    ALTER COLUMN message_kind SET NOT NULL,
    ALTER COLUMN message_type SET NOT NULL,
    ALTER COLUMN message_version SET NOT NULL,
    ALTER COLUMN message_id SET NOT NULL,
    ALTER COLUMN subject SET NOT NULL,
    ADD CONSTRAINT outbox_message_kind_valid
        CHECK (message_kind IN ('command', 'event')),
    ADD CONSTRAINT outbox_message_version_nonnegative
        CHECK (message_version >= 0);
