//! FileTransferService — the object transfer surface: direct upload
//! (validate → sniff → store → record) and download. The object store
//! is MinIO (S3) when `oss.yaml` is configured, else the local mirror
//! with identical metadata semantics; the upload applies the media
//! rules from `super::media` (≤50 MiB, content-sniffed MIME whitelist,
//! `bucket = images|videos|audios|docs|files` by type, object name
//! `dir/uuid.ext`, sha256 content hash, guid v7).

use std::sync::Arc;

use sea_orm::sea_query::Condition;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::state::{
    db_err, internal_error, not_found, operator_of, status_error, AppState, StatusError,
};
use proto::proto::storage::service::v1::{
    DownloadFileRequest, DownloadFileResponse, UploadFileRequest, UploadFileResponse,
};
use rushwind_oss::ObjectStorage as _;

use super::local_store;
use super::media::{bucket_for_mime, mime_allowed, signed_media_url, sniff_mime, MAX_UPLOAD_SIZE};

pub struct FileTransferService {
    pub state: Arc<AppState>,
}

impl FileTransferService {
    /// directUploadFile: validate → sniff → store → record.
    async fn upload(
        &self,
        ctx: &rushwind_http_binding::ctx::RequestContext,
        req: &UploadFileRequest,
    ) -> Result<UploadFileResponse, StatusError> {
        let payload = operator_of(ctx)?;
        let storage_object = req
            .storage_object
            .as_ref()
            .ok_or_else(|| status_error("BAD_REQUEST", "storage object required"))?;
        let bytes = match &req.source {
            Some(proto::proto::storage::service::v1::upload_file_request::Source::File(bytes)) => {
                bytes.clone()
            }
            _ => {
                return Err(status_error(
                    "BAD_REQUEST",
                    "file content required (presigned flow disabled)",
                ))
            }
        };
        if bytes.len() > MAX_UPLOAD_SIZE {
            return Err(status_error(
                "BAD_REQUEST",
                "file exceeds the 50MiB upload limit",
            ));
        }
        let mut mime = req.mime.clone().unwrap_or_default();
        if mime.is_empty() {
            return Err(status_error("BAD_REQUEST", "unknown mime type"));
        }
        if req
            .source_file_name
            .as_deref()
            .map(str::is_empty)
            .unwrap_or(true)
        {
            return Err(status_error("BAD_REQUEST", "unknown source file name"));
        }
        if !mime_allowed(&mime) {
            return Err(status_error(
                "BAD_REQUEST",
                format!("mime type [{mime}] is not allowed"),
            ));
        }
        // The bytes decide the type, not the client declaration — the
        // sniffed type overrides so the bucket route cannot be bypassed
        // by a relabelled payload.
        if let Some(sniffed) = sniff_mime(&bytes) {
            mime = sniffed.to_string();
        }
        let dir = storage_object
            .file_directory
            .clone()
            .filter(|d| !d.is_empty() && !d.starts_with('/') && !d.contains(".."))
            .unwrap_or_else(|| "uploads".into());
        // Content hash (sha256) — the dedup/fingerprint.
        let hash = crate::crypto::sha256_hex(&bytes);
        let ext = req
            .source_file_name
            .as_deref()
            .and_then(|n| n.rsplit('.').next())
            .map(|e| e.to_string())
            .unwrap_or_else(|| "bin".into());
        let guid = uuid::Uuid::now_v7().simple().to_string();
        let save_name = format!("{guid}.{ext}");
        let bucket = bucket_for_mime(&mime);
        let object_name = format!("{dir}/{save_name}");

        // The object store: MinIO when configured (the URL shapes the
        // reference pins — link carries the download host), else the
        // local mirror with identical metadata semantics.
        let (link_url, public_url) = match self.state.oss.as_ref() {
            Some(oss) => {
                let storage = oss
                    .storage(bucket)
                    .ok_or_else(|| internal_error("storage engine missing"))?;
                storage
                    .put(&object_name, &bytes, Some(mime.as_str()))
                    .await
                    .map_err(|e| internal_error(format!("storage put: {e}")))?;
                let download_url = format!(
                    "{}/{bucket}/{object_name}",
                    oss.download_host.trim_end_matches('/')
                );
                let storage_path = format!("/{bucket}/{object_name}");
                let public_url = signed_media_url(crate::crypto::crypto_key(), &storage_path);
                (download_url, public_url)
            }
            None => {
                local_store(bucket)
                    .put(&object_name, &bytes, Some(mime.as_str()))
                    .await
                    .map_err(|e| internal_error(format!("storage put: {e}")))?;
                (object_name.clone(), String::new())
            }
        };

        let row = crate::data::files::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            provider: Set(Some("MINIO".into())),
            bucket_name: Set(Some(bucket.into())),
            file_directory: Set(Some(dir)),
            file_guid: Set(Some(guid)),
            save_file_name: Set(Some(save_name)),
            file_name: Set(req.source_file_name.clone()),
            extension: Set(Some(ext)),
            size: Set(Some(bytes.len() as i64)),
            size_format: Set(Some(human_size(bytes.len() as u64))),
            link_url: Set(Some(link_url)),
            content_hash: Set(Some(hash)),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;

        Ok(UploadFileResponse {
            object_name: Some(row.link_url.clone().unwrap_or_default()),
            presigned_url: None,
            public_url: Some(public_url),
        })
    }
}

#[async_trait::async_trait]
impl proto::gen::services::FileTransferServiceHandlers for FileTransferService {
    async fn download_file(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: DownloadFileRequest,
    ) -> Result<DownloadFileResponse, StatusError> {
        // Selector: file_id (direct record), storage_object (bucket+key
        // download_url: local mirror resolution only;
        // only same-origin local mirrors resolve here).
        let row = match &req.selector {
            Some(proto::proto::storage::service::v1::download_file_request::Selector::FileId(
                id,
            )) => crate::data::files::Entity::find_by_id(*id as i64)
                .one(&self.state.db)
                .await
                .map_err(db_err)?,
            Some(
                proto::proto::storage::service::v1::download_file_request::Selector::StorageObject(
                    obj,
                ),
            ) => crate::data::files::Entity::find()
                .filter(
                    Condition::all()
                        .add(
                            crate::data::files::Column::BucketName
                                .eq(obj.bucket_name.clone().unwrap_or_default()),
                        )
                        .add(
                            crate::data::files::Column::LinkUrl
                                .eq(obj.object_name.clone().unwrap_or_default()),
                        ),
                )
                .one(&self.state.db)
                .await
                .map_err(db_err)?,
            Some(
                proto::proto::storage::service::v1::download_file_request::Selector::DownloadUrl(_),
            ) => {
                return Err(status_error(
                    "UNIMPLEMENTED",
                    "remote url download not wired",
                ));
            }
            None => None,
        }
        .ok_or_else(|| not_found("file"))?;
        let bucket = row.bucket_name.clone().unwrap_or_else(|| "files".into());
        let mut object_key = row.file_directory.clone().unwrap_or_default();
        if !object_key.is_empty() {
            object_key.push('/');
        }
        object_key.push_str(row.save_file_name.as_deref().unwrap_or_default());
        let (bytes, storage_path) = match self.state.oss.as_ref() {
            Some(oss) => {
                let storage = oss
                    .storage(&bucket)
                    .ok_or_else(|| internal_error("storage engine missing"))?;
                let bytes = storage
                    .get(&object_key)
                    .await
                    .map_err(|e| internal_error(format!("storage get: {e}")))?;
                (bytes, format!("/{bucket}/{object_key}"))
            }
            None => {
                let bytes = local_store(&bucket)
                    .get(&object_key)
                    .await
                    .map_err(|e| internal_error(format!("storage get: {e}")))?;
                (bytes, format!("./data/files/{bucket}/{object_key}"))
            }
        };
        let mime = match row.extension.as_deref() {
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("pdf") => "application/pdf",
            Some("txt") => "text/plain",
            _ => "application/octet-stream",
        };
        Ok(DownloadFileResponse {
            source_file_name: row.file_name.clone().unwrap_or_default(),
            mime: mime.into(),
            size: bytes.len() as i64,
            checksum: row.content_hash.clone().unwrap_or_default(),
            storage_path,
            updated_at: row.updated_at.and_then(crate::state::naive_to_ts),
            content: Some(
                proto::proto::storage::service::v1::download_file_response::Content::File(bytes),
            ),
        })
    }

    async fn put_upload_file(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UploadFileRequest,
    ) -> Result<UploadFileResponse, StatusError> {
        self.upload(&ctx, &req).await
    }

    async fn post_upload_file(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UploadFileRequest,
    ) -> Result<UploadFileResponse, StatusError> {
        self.upload(&ctx, &req).await
    }
}

fn human_size(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < units.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.2}{}", units[unit])
}
