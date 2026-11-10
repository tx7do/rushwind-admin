//! admin build — the contract build over the app's own `api/`
//! workspace (biz-proto-build, the manifest_dir/api layout): the buf
//! workspace compiles the admin contract tree AND the fork's module
//! trees into ONE annotated closure (one pool) and ONE filtered types
//! set (one type per proto package), then emits one gen face per
//! source tree. Each face builds its own module dir (`buf build
//! <dir>`), so the admin face's route table stays the synced admin
//! registration set and a fork face stays its own tree.

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/auth_free.rs"));

fn main() -> Result<(), Box<dyn std::error::Error>> {
    biz_proto_build::run(biz_proto_build::Build {
        manifest_dir: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        dep_data_files: &["pagination/v1/pagination.proto"],
        extra_type_roots: &["fork/protos"],
        auth_free: AUTH_FREE,
        redact_plan_expr: Some("crate::redact_plan()"),
        proto_module_path: "crate::proto",
        pool_expr: "crate::pool()",
        faces: &[
            biz_proto_build::Face {
                label: "admin",
                module_file: "admin_gen.rs",
                root_prefix: None,
                source_dir: Some("protos"),
                module_path: Some("crate::proto::gen::"),
            },
            biz_proto_build::Face {
                label: "template",
                module_file: "template_gen.rs",
                // Sliced from the full closure by tree prefix: a second
                // fork module joining `api/fork/protos/` must not leak
                // into this face's route table (the copy-rename drill
                // compiled a second module against this shape and the
                // guards stayed green — the drill itself is deleted,
                // per house style).
                root_prefix: Some("notice/"),
                source_dir: None,
                module_path: Some("crate::proto::gen::template::"),
            },
        ],
    })
}

