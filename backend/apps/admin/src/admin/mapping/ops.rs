//! The ops wire enums: task scheduling semantics (`task` proto domain)
//! and the script runtime language (`script` proto domain).

enum_map!(task_type_of {
    "DELAY" => 1,
    "WAIT_RESULT" => 2,
});

enum_map!(script_language_of + script_language_str {
    "JAVASCRIPT" => 1,
});
