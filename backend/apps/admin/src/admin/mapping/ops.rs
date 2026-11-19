//! The ops wire enums: task scheduling semantics (`task` proto domain)
//! and the script runtime language (`script` proto domain).

enum_map!(task_type_of => "task.service.v1.Task.Type");

enum_map!(script_language_of + script_language_str => "script.service.v1.Language");
