//! The permission-domain wire enums: the menu tree's node types and
//! the module partition shared with the api-catalog.

// menu_module is shared with the api-catalog's business_module column.

enum_map!(menu_type_of + menu_type_str => "permission.service.v1.Menu.Type");

enum_map!(menu_module_of + menu_module_str => "identity.service.v1.Module");
