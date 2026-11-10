//! FileService — the file metadata surface: CRUD over the `files`
//! table through the generated handlers. The transfer surface
//! (upload/download) lives in [`transfer`], the media rules (MIME
//! whitelist, sniffing, bucket routing, signed URLs, the image proxy)
//! in [`media`]. The physical object store is MinIO (S3) when
//! `oss.yaml` is configured, else the object bytes land on the local
//! data directory (`./data/files`) with identical metadata semantics.

mod media;
mod transfer;

pub use media::image_proxy;
pub use transfer::FileTransferService;

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::mapping;
use crate::state::{db_err, not_found, operator_of, AppState, StatusError};
use pbjson_types::Empty;
use proto::proto::pagination::PagingRequest;
use proto::proto::storage::service::v1::{
    CreateFileRequest, DeleteFileRequest, File, GetFileRequest, ListFileResponse, UpdateFileRequest,
};
use rushwind_oss::ObjectStorage as _;

/// The local-disk store for one bucket — the no-endpoint profile's
/// engine, rooted at the data directory's bucket folder (the framework
/// engine owns the fs details: directory materialization, the NotFound
/// read, the idempotent delete).
fn local_store(bucket: &str) -> rushwind_oss_local::LocalStorage {
    rushwind_oss_local::LocalStorage::new(std::path::Path::new("./data/files").join(bucket))
}

/// Unknown rows read as the zero provider (LOCAL).
fn provider_to_proto(s: &str) -> i32 {
    mapping::file_provider_of(s).unwrap_or(0)
}

fn file_proto(r: crate::data::files::Model) -> File {
    File {
        id: Some(r.id as u64),
        provider: r.provider.as_deref().map(provider_to_proto),
        bucket_name: r.bucket_name,
        file_directory: r.file_directory,
        file_guid: r.file_guid,
        save_file_name: r.save_file_name,
        file_name: r.file_name,
        extension: r.extension,
        size: r.size.map(|v| v as u64),
        size_format: r.size_format,
        link_url: r.link_url,
        content_hash: r.content_hash,
        tenant_id: r.tenant_id.map(|v| v as u64),
        tenant_name: None,
        created_by: r.created_by.map(|v| v as u64),
        updated_by: r.updated_by.map(|v| v as u64),
        deleted_by: r.deleted_by.map(|v| v as u64),
        created_at: r.created_at.and_then(crate::state::naive_to_ts),
        updated_at: r.updated_at.and_then(crate::state::naive_to_ts),
        deleted_at: r.deleted_at.and_then(crate::state::naive_to_ts),
    }
}

pub struct FileService {
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl proto::gen::services::FileServiceHandlers for FileService {
    async fn list(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: PagingRequest,
    ) -> Result<ListFileResponse, StatusError> {
        let repo =
            crate::data::repos::FileRepo::new(&self.state.db, crate::data::Viewer::from_ctx(&ctx));
        let (rows, total) = repo.paged_list(&req).await?;
        Ok(ListFileResponse {
            items: rows.into_iter().map(file_proto).collect(),
            total,
        })
    }

    async fn get(
        &self,
        _ctx: rushwind_http_binding::ctx::RequestContext,
        req: GetFileRequest,
    ) -> Result<File, StatusError> {
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::storage::service::v1::get_file_request::QueryBy
        );
        let row = crate::data::files::Entity::find_by_id(id as i64)
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("file"))?;
        Ok(file_proto(row))
    }

    async fn create(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: CreateFileRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let data = crate::state::require_data(req.data)?;
        crate::data::files::ActiveModel {
            tenant_id: Set(Some(payload.tenant_id)),
            provider: Set(Some("MINIO".into())),
            bucket_name: Set(data.bucket_name),
            file_directory: Set(data.file_directory),
            file_guid: Set(data.file_guid),
            save_file_name: Set(data.save_file_name),
            file_name: Set(data.file_name),
            extension: Set(data.extension),
            size: Set(data.size.map(|v| v as i64)),
            size_format: Set(data.size_format),
            link_url: Set(data.link_url),
            content_hash: Set(data.content_hash),
            created_by: Set(Some(payload.user_id)),
            created_at: Set(Some(crate::data::now())),
            updated_at: Set(Some(crate::data::now())),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .map_err(db_err)?;
        Ok(Empty {})
    }

    async fn update(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: UpdateFileRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let row = crate::data::files::Entity::find_by_id(req.id as i64)
            .filter(crate::data::files::Column::TenantId.eq(payload.tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("file"))?;
        let mut a: crate::data::files::ActiveModel = row.into();
        if let Some(data) = &req.data {
            if let Some(v) = &data.file_name {
                a.file_name = Set(Some(v.clone()));
            }
        }
        crate::stamp_update!(a, payload.user_id);
        a.update(&self.state.db).await.map_err(db_err)?;
        Ok(Empty {})
    }

    async fn delete(
        &self,
        ctx: rushwind_http_binding::ctx::RequestContext,
        req: DeleteFileRequest,
    ) -> Result<Empty, StatusError> {
        let payload = operator_of(&ctx)?;
        let id = crate::query_by_id!(
            req.query_by,
            proto::proto::storage::service::v1::delete_file_request::QueryBy
        );
        let row = crate::data::files::Entity::find_by_id(id as i64)
            .filter(crate::data::files::Column::TenantId.eq(payload.tenant_id))
            .one(&self.state.db)
            .await
            .map_err(db_err)?
            .ok_or_else(|| not_found("file"))?;
        // Physical object removal (best-effort on either store).
        let bucket = row.bucket_name.clone().unwrap_or_else(|| "files".into());
        let mut object_key = row.file_directory.clone().unwrap_or_default();
        if !object_key.is_empty() {
            object_key.push('/');
        }
        object_key.push_str(row.save_file_name.as_deref().unwrap_or_default());
        if let Some(oss) = self.state.oss.as_ref().and_then(|o| o.storage(&bucket)) {
            let _ = oss.delete(&object_key).await;
        } else {
            let _ = local_store(&bucket).delete(&object_key).await;
        }
        crate::data::files::Entity::delete_by_id(row.id)
            .exec(&self.state.db)
            .await
            .map_err(db_err)?;
        Ok(Empty {})
    }
}
