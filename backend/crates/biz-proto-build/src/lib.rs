//! The contract build engine for the business modules — the
//! [`rushwind_proto_build`] pipeline with ONE layout difference: the
//! buf workspace (buf.yaml, buf.lock, protos/) sits inside the module
//! crate itself (`<module>/api`), not two levels up beside a shared
//! backend root. A business module carries its own contract tree, so
//! a copy-rename of the module directory moves the contract with it.
//!
//! Everything else mirrors the framework engine verbatim, for the same
//! reasons:
//!
//! * the **annotated descriptor set** — the FULL compile closure
//!   including the annotation declarations (google.api.http,
//!   errors.code, redact, validate, gnostic) — the raw source-free
//!   `buf build` image. buf, not a Rust-side compiler: a Rust-side
//!   serializer drops custom-option bytes, which are the entire point
//!   of this set. Requires `buf` on PATH; the workspace's buf.lock
//!   pins the dependency commits, fetched into the module cache on
//!   first use.
//! * the **Rust types** — prost + pbjson from a FILTERED descriptor
//!   set: a second buf build (WITH source info — the proto comments
//!   become the generated types' rustdoc) decoded and pruned to
//!   data-carrying files (the contract tree's own top-level modules,
//!   the declared dependency data files, the well-known types
//!   referenced as field types). Annotation-only files are dropped so
//!   prost/pbjson never emit types for them.
//! * the [`rushwind_gen_http`] route/binding/trait/mount surface —
//!   once per configured [`Face`], optionally sliced from the
//!   annotated closure by `rushwind_proto_build::slice::sub_closure`'s
//!   byte-verbatim reachability walk and re-pointed from `crate::gen::`
//!   to the face's own module.
//!
//! A module build.rs reduces to a [`Build`] value: the module dir, the
//! dependency data files, the auth-free whitelist, the gen config, and
//! the face list.
//!
//! Provenance note: this crate retires when the upstream engine grows
//! an api-root knob — until then it tracks the engine's shape by hand.

#![forbid(unsafe_code)]

mod slice;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use prost::Message as _;
use prost_types::FileDescriptorSet;

/// The module's contract build: everything the engine needs from the
/// caller, expressed as config.
pub struct Build {
    /// The module crate's own directory (`env!("CARGO_MANIFEST_DIR")`
    /// in the module's build.rs) — the engine derives the buf
    /// workspace from `<module>/api` (buf.yaml, buf.lock, protos/).
    pub manifest_dir: PathBuf,
    /// Data-carrying dependency files, by import path
    /// (`pagination/v1/pagination.proto`) — real runtime types (the
    /// paging envelope) that participate in type generation. Their
    /// presence in the compiled closure is verified, so a dependency
    /// dropped from the workspace fails the build instead of silently
    /// losing the type.
    pub dep_data_files: &'static [&'static str],
    /// The module's auth-free whitelist — single-sourced from the
    /// crate's `src/auth_free.rs`, included at build-script scope.
    pub auth_free: &'static [(&'static str, &'static str)],
    /// The generated mounts' redaction-plan expression (`None` threads
    /// a literal `None` — no static redaction).
    pub redact_plan_expr: Option<&'static str>,
    /// The generated types' Rust module path (the modules' generated
    /// types resolve through `crate::proto`).
    pub proto_module_path: &'static str,
    /// The descriptor-pool expression the mounts hand the glue.
    pub pool_expr: &'static str,
    /// The gen faces: one entry per output module.
    pub faces: &'static [Face],
}

/// One generated route/binding/trait/mount module.
pub struct Face {
    /// Diagnostics label (`template`, `erp`, …).
    pub label: &'static str,
    /// The output file name inside `OUT_DIR` (`template_gen.rs`, …).
    pub module_file: &'static str,
    /// The root prefix slicing the annotated closure to this face's
    /// sub-closure (`template/service/v1/`); `None` generates over the
    /// full closure.
    pub root_prefix: Option<&'static str>,
    /// The generated self-references (`crate::gen::` — the generator's
    /// hardcoded module path) are rewritten to this path when present;
    /// `None` keeps `crate::gen::` (the single-face module's own
    /// module).
    pub module_path: Option<&'static str>,
}

/// Runs the full contract build. See the crate docs for the shape.
pub fn run(build: Build) -> Result<(), Box<dyn std::error::Error>> {
    // The buf workspace sits inside the module crate itself: a
    // copy-rename of the module directory moves the contract with it.
    let api_root = build.manifest_dir.join("api");
    let proto_root = api_root.join("protos");

    // The contract tree's own top-level modules — derived from the tree
    // itself so a new module directory needs no whitelist edit. The types
    // filter keeps a file when its top-level segment is one of these
    // modules, or it is an explicitly declared dependency data file, or a
    // well-known import.
    let mut contract_tops: Vec<String> = fs::read_dir(&proto_root)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    contract_tops.sort();
    if contract_tops.is_empty() {
        return Err(format!("no contract protos found under {}", proto_root.display()).into());
    }
    let is_types_kept = |name: &str| -> bool {
        match name.split_once('/') {
            Some((top, _)) => {
                contract_tops.iter().any(|t| t == top)
                    || build.dep_data_files.contains(&name)
                    || name.starts_with(WELL_KNOWN_PREFIX)
            }
            None => build.dep_data_files.contains(&name) || name.starts_with(WELL_KNOWN_PREFIX),
        }
    };

    // The annotated full closure via buf build (option bytes preserved,
    // imports resolved through the module workspace: its own modules and
    // the buf.lock-pinned dependency commits, fetched into the module
    // cache on first use). Output ordering is buf-determined (sorted),
    // keeping the generated ROUTES table platform-independent.
    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let annotated_path = out_dir.join("annotated_descriptor.bin");
    let mut cmd = Command::new("buf");
    cmd.current_dir(&api_root)
        .args(["build", "--exclude-source-info", "--output"])
        .arg(&annotated_path);
    let output = cmd.output().map_err(|e| format!("buf: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "buf build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let annotated = fs::read(&annotated_path)?;
    let annotated_size = annotated.len();

    // The types closure: the same workspace compiled WITH source info —
    // prost turns the proto comments into rustdoc on the generated types.
    // The decode runs through prost, so the buf image's bufExtension
    // bookkeeping drops out — a plain FileDescriptorSet.
    let types_image_path = out_dir.join("types_source_image.binpb");
    let mut types_cmd = Command::new("buf");
    types_cmd
        .current_dir(&api_root)
        .args(["build", "--output"])
        .arg(&types_image_path);
    let output = types_cmd.output().map_err(|e| format!("buf: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "buf build (types closure) failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let types_image = fs::read(&types_image_path)?;
    let _ = fs::remove_file(&types_image_path);
    let full_fds = FileDescriptorSet::decode(types_image.as_slice())
        .map_err(|e| format!("the buf image does not decode as a FileDescriptorSet: {e}"))?;
    for rel in build.dep_data_files {
        if !full_fds.file.iter().any(|f| f.name() == *rel) {
            return Err(format!(
                "dependency data file missing from the compiled closure: {rel} \
                 (declared in the module workspace's deps and pinned by its buf.lock?)"
            )
            .into());
        }
    }

    // Filtered set for type generation: drop annotation-only files, prune
    // dependency edges to kept files.
    let mut types_fds = full_fds;
    types_fds.file.retain(|f| is_types_kept(f.name()));
    for f in &mut types_fds.file {
        let deps: Vec<String> = f.dependency.to_vec();
        f.dependency = deps
            .into_iter()
            .filter(|d| is_types_kept(d.as_str()))
            .collect();
    }
    let types_bytes = types_fds.encode_to_vec();

    // Sweep stale codegen artifacts (earlier runs may have written packages
    // that the filter has since dropped).
    for entry in fs::read_dir(&out_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "rs").unwrap_or(false) {
            let _ = fs::remove_file(&path);
        }
    }

    eprintln!(
        "[biz-proto-build] annotated closure: {annotated_size} bytes (buf build); \
         types set: {} files (annotation declarations dropped)",
        types_fds.file.len()
    );

    // The pbjson serde impls' package list, derived from the filtered set
    // itself (dir names do not map 1:1 to package names); well-knowns are
    // externed and excluded.
    let mut packages: Vec<String> = types_fds
        .file
        .iter()
        .filter(|f| !is_well_known(f.name()))
        .map(|f| format!(".{}", f.package()))
        .collect();
    packages.sort();
    packages.dedup();
    let pkg_refs: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();

    // prost type generation straight from the filtered set — compile_fds
    // touches neither protoc nor the file system, so the types face
    // carries no input paths at all.
    let mut config = prost_build::Config::new();
    config
        .compile_well_known_types()
        .extern_path(".google.protobuf", "::pbjson_types")
        .include_file("proto_include.rs");
    config.compile_fds(types_fds)?;

    // pbjson serde impls over the same filtered set's bytes.
    pbjson_build::Builder::new()
        .register_descriptors(&types_bytes)?
        .build(&pkg_refs)?;

    // The generated route/binding/trait/mount surface: the framework
    // generator (rushwind-gen-http) over the annotated closure — once
    // per configured face.
    let cfg = rushwind_gen_http::CodegenConfig {
        proto_module_path: build.proto_module_path,
        pool_expr: build.pool_expr,
        auth_free: build.auth_free,
        redact_plan_expr: build.redact_plan_expr,
    };
    for face in build.faces {
        let bytes = match face.root_prefix {
            Some(prefix) => {
                let sub = slice::sub_closure(&annotated, prefix).map_err(|e| e.to_string())?;
                eprintln!(
                    "[biz-proto-build] {} face sub-closure: {} bytes (full: {})",
                    face.label,
                    sub.len(),
                    annotated.len()
                );
                sub
            }
            None => annotated.clone(),
        };
        let src = match rushwind_gen_http::generate_from_bytes(&bytes, &cfg) {
            Ok(src) => src,
            Err(e) => panic!("gen-rust code generation failed ({} face): {e}", face.label),
        };
        let src = match face.module_path {
            Some(module) => src.replace("crate::gen::", module),
            None => src,
        };
        fs::write(out_dir.join(face.module_file), src)?;
    }

    // Re-run on any contract change (and on the auth-free table, and on
    // the dependency pin set).
    println!(
        "cargo:rerun-if-changed={}",
        build.manifest_dir.join("src/auth_free.rs").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        api_root.join("buf.yaml").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        api_root.join("buf.lock").display()
    );
    println!("cargo:rerun-if-changed={}", proto_root.display());

    Ok(())
}

/// Well-known types referenced as field types by the contract tree.
/// Externed to `pbjson_types` by the prost config; kept in the filtered
/// descriptor set so references resolve, excluded from pbjson output.
const WELL_KNOWN_PREFIX: &str = "google/protobuf/";

fn is_well_known(name: &str) -> bool {
    name.starts_with(WELL_KNOWN_PREFIX)
}
