//! The internal_message-domain wire enums: publish lifecycle, per-
//! recipient state and the message kind.

enum_map!(internal_message_status_of {
    "PUBLISHED" => 1,
    "SCHEDULED" => 2,
    "REVOKED" => 3,
    "ARCHIVED" => 4,
    "DELETED" => 5,
});

enum_map!(internal_message_recipient_status_of {
    "READ" => 2,
    "REVOKED" => 3,
    "DELETED" => 4,
});

enum_map!(internal_message_type_of {
    "PRIVATE" => 1,
    "GROUP" => 2,
});
