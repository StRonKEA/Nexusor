CREATE TABLE interrupted_outputs (
    conversation_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    source_user_id TEXT NOT NULL,
    source_user_text TEXT NOT NULL,
    source_message_id TEXT,
    PRIMARY KEY (conversation_id, message_id),
    FOREIGN KEY (conversation_id, message_id)
        REFERENCES messages(conversation_id, message_id) ON DELETE CASCADE
);
