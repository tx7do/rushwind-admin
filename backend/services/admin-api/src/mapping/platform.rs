//! The platform-wide wire enums: config value types (`config` proto
//! domain) and the file storage provider (`storage` proto domain) —
//! the same pairing [`crate::data`] keeps in its `platform` entity file.

enum_map!(config_value_type_of {
    "BOOL" => 2,
    "INT" => 3,
});

enum_map!(file_provider_of {
    "MINIO" => 1,
});
