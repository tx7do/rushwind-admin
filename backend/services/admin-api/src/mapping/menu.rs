//! The permission-domain wire enums: the menu tree's node types and
//! the module partition shared with the api-catalog.

// menu_module is shared with the api-catalog's business_module column.

enum_map!(menu_type_of + menu_type_str {
    "CATALOG" => 0,
    "BUTTON" => 2,
    "EMBEDDED" => 3,
    "LINK" => 4,
});

enum_map!(menu_module_of + menu_module_str {
    "DASHBOARD" => 1,
    "OPM" => 2,
    "SYSTEM" => 3,
    "DICT" => 4,
    "TENANT" => 5,
    "PERMISSION" => 6,
    "LOG" => 7,
    "INTERNAL_MESSAGE" => 8,
    "FILE" => 9,
    "TASK" => 10,
});
