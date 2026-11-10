//! module-template build — the module's own contract build over
//! biz-proto-build: the buf workspace is this crate's own `api/`
//! directory, the face slices the annotated closure to the module's
//! tree (`notice/service/v1/`), and the gen surface lands in
//! `OUT_DIR/template_gen.rs`.

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/auth_free.rs"));

fn main() -> Result<(), Box<dyn std::error::Error>> {
    biz_proto_build::run(biz_proto_build::Build {
        manifest_dir: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        dep_data_files: &["pagination/v1/pagination.proto"],
        auth_free: AUTH_FREE,
        redact_plan_expr: None,
        proto_module_path: "crate::proto",
        pool_expr: "crate::pool()",
        faces: &[biz_proto_build::Face {
            label: "template",
            module_file: "template_gen.rs",
            root_prefix: Some("notice/service/v1/"),
            module_path: None,
        }],
    })
}
