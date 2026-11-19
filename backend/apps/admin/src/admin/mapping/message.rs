//! The internal_message-domain wire enums: publish lifecycle, per-
//! recipient state and the message kind.

enum_map!(internal_message_status_of => "internal_message.service.v1.InternalMessage.Status");

enum_map!(internal_message_recipient_status_of => "internal_message.service.v1.InternalMessageRecipient.Status");

enum_map!(internal_message_type_of => "internal_message.service.v1.InternalMessage.Type");
