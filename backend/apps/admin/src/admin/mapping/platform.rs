//! The platform-wide wire enums: config value types (`config` proto
//! domain) and the file storage provider (`storage` proto domain) —
//! the same pairing [`crate::admin::data`] keeps in its `platform` entity file.

enum_map!(config_value_type_of => "config.service.v1.Config.ConfigValueType");

enum_map!(file_provider_of => "storage.service.v1.OSSProvider");
