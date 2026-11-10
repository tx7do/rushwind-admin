//! The byte-level annotated-closure slicer — one module face's compile
//! sub-closure.
//!
//! Vendored verbatim from `rushwind-proto-build/src/slice.rs` (rev
//! 08fbfcd), where the function is `pub(crate)`: the framework engine
//! does not expose it, so this crate carries the copy until the
//! upstream engine grows an api-root knob / public slice surface. The
//! body is byte-identical to upstream (tests included).
//!
//! The slice works at the BYTE level and never re-encodes a
//! FileDescriptorProto: prost's decoder discards unknown fields, which is
//! exactly where the custom-option bytes (google.api.http / errors.code)
//! ride, so a decode+re-encode round-trip would strip every annotation.
//! Each file record is copied verbatim; only the top-level record set is
//! filtered. Dependencies of retained files are retained by construction
//! (the reachability walk adds them), so no dependency-list rewrite is
//! needed.

/// Slices the annotated closure to the files reachable (transitively
/// through `dependency` edges) from the files whose names start with
/// `root_prefix`.
pub(crate) fn sub_closure(annotated: &[u8], root_prefix: &str) -> Result<Vec<u8>, String> {
    // Split the FileDescriptorSet into per-file records, harvesting each
    // record's name (field 1) and dependencies (field 3) with a minimal
    // walk that leaves every other field's bytes untouched.
    let mut files: Vec<(Vec<u8>, String, Vec<String>)> = Vec::new();
    let mut top = records(annotated)?;
    for (field, wire, payload) in top.drain(..) {
        if field != 1 || wire != 2 {
            continue;
        }
        let mut name = String::new();
        let mut deps = Vec::new();
        for (field, wire, value) in records(payload)? {
            match (field, wire) {
                (1, 2) if name.is_empty() => {
                    name = String::from_utf8_lossy(value).into_owned();
                }
                (3, 2) => deps.push(String::from_utf8_lossy(value).into_owned()),
                _ => {}
            }
        }
        files.push((payload.to_vec(), name, deps));
    }

    // Reachability from the root prefix.
    let mut keep = std::collections::HashSet::new();
    let mut queue: Vec<String> = files
        .iter()
        .map(|(_, name, _)| name.clone())
        .filter(|n| n.starts_with(root_prefix))
        .collect();
    let deps_of = |name: &str| -> &[String] {
        files
            .iter()
            .find(|(_, n, _)| n == name)
            .map(|(_, _, d)| d.as_slice())
            .unwrap_or(&[])
    };
    while let Some(name) = queue.pop() {
        if keep.insert(name.clone()) {
            queue.extend(deps_of(&name).iter().cloned());
        }
    }

    // Re-emit the retained records verbatim (original order, field-1
    // length-delimited framing).
    let mut out = Vec::with_capacity(annotated.len());
    for (blob, name, _) in &files {
        if keep.contains(name) {
            write_tag(&mut out, 1, 2);
            write_varint(&mut out, blob.len() as u64);
            out.extend_from_slice(blob);
        }
    }
    Ok(out)
}

/// One wire-level record: (field number, wire type, payload slice).
type Record<'a> = (u32, u8, &'a [u8]);

/// The wire-level record walk: yields `(field number, wire type, payload)`
/// for length-delimited records, or empty payloads after consuming
/// scalar records.
fn records(buf: &[u8]) -> Result<Vec<Record<'_>>, String> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        let key = read_varint(buf, &mut pos)?;
        let field = (key >> 3) as u32;
        let wire = (key & 7) as u8;
        match wire {
            0 => {
                read_varint(buf, &mut pos)?;
                out.push((field, wire, &[][..]));
            }
            1 => {
                let end = pos.checked_add(8).filter(|e| *e <= buf.len());
                out.push((field, wire, buf.get(pos..end.unwrap_or(pos)).unwrap_or(&[])));
                pos = end.ok_or("truncated fixed64")?;
            }
            2 => {
                let len = read_varint(buf, &mut pos)? as usize;
                let end = pos
                    .checked_add(len)
                    .filter(|e| *e <= buf.len())
                    .ok_or("truncated length-delimited record")?;
                out.push((field, wire, &buf[pos..end]));
                pos = end;
            }
            5 => {
                let end = pos.checked_add(4).filter(|e| *e <= buf.len());
                out.push((field, wire, buf.get(pos..end.unwrap_or(pos)).unwrap_or(&[])));
                pos = end.ok_or("truncated fixed32")?;
            }
            other => return Err(format!("unsupported wire type {other}")),
        }
    }
    Ok(out)
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Result<u64, String> {
    let mut result = 0u64;
    let mut shift = 0;
    loop {
        let b = *buf.get(*pos).ok_or("truncated varint")?;
        *pos += 1;
        result |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Ok(result);
        }
        shift += 7;
        if shift >= 64 {
            return Err("varint too long".into());
        }
    }
}

fn write_tag(out: &mut Vec<u8>, field: u32, wire: u8) {
    write_varint(out, (u64::from(field) << 3) | u64::from(wire));
}

fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-frames a FileDescriptorSet: one field-1 record per file,
    /// each payload carrying the name (field 1) and dependency list
    /// (field 3) — plus an unknown field (a pretend option byte) that
    /// must survive the slice verbatim.
    fn frame(files: &[(&str, &[&str], &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, deps, unknown) in files.iter() {
            let mut payload = Vec::new();
            write_tag(&mut payload, 1, 2);
            write_varint(&mut payload, name.len() as u64);
            payload.extend_from_slice(name.as_bytes());
            for &dep in deps.iter() {
                write_tag(&mut payload, 3, 2);
                write_varint(&mut payload, dep.len() as u64);
                payload.extend_from_slice(dep.as_bytes());
            }
            payload.extend_from_slice(unknown);
            write_tag(&mut out, 1, 2);
            write_varint(&mut out, payload.len() as u64);
            out.extend_from_slice(&payload);
        }
        out
    }

    #[test]
    fn reachability_keeps_transitive_deps_with_bytes_verbatim() {
        let option_bytes = [0xfa, 0x02, 0x03, 0xaa, 0xbb, 0xcc]; // field 63, len 3
        let annotated = frame(&[
            (
                "admin/service/v1/a.proto",
                &["common/v1/c.proto"],
                &option_bytes,
            ),
            ("app/service/v1/x.proto", &["common/v1/c.proto"], &[]),
            ("common/v1/c.proto", &["google/protobuf/empty.proto"], &[]),
            ("unrelated/v1/z.proto", &[], &[]),
        ]);

        let admin = sub_closure(&annotated, "admin/service/v1/").unwrap();
        let text = admin.iter().map(|b| *b as char).collect::<String>();
        assert!(text.contains("admin/service/v1/a.proto"));
        assert!(text.contains("common/v1/c.proto"), "transitive dep rides");
        assert!(text.contains("google/protobuf/empty.proto"));
        assert!(!text.contains("app/service/v1/x.proto"));
        assert!(!text.contains("unrelated/v1/z.proto"));
        // The unknown field (option bytes) survives byte-verbatim.
        assert!(
            admin.windows(option_bytes.len()).any(|w| w == option_bytes),
            "option bytes must not be stripped"
        );

        // The app face reaches the shared dep through its own root.
        let app = sub_closure(&annotated, "app/service/v1/").unwrap();
        let text = app.iter().map(|b| *b as char).collect::<String>();
        assert!(text.contains("app/service/v1/x.proto"));
        assert!(!text.contains("admin/service/v1/a.proto"));
    }

    #[test]
    fn empty_prefix_keeps_nothing() {
        let annotated = frame(&[("a.proto", &[], &[])]);
        assert!(sub_closure(&annotated, "nope/").unwrap().is_empty());
    }
}
