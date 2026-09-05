use axum::{
    extract::{Path, State},
    Json,
};
use serde_json::{json, Value};

use crate::StorageState;
use freebuff_shared::AppError;

pub async fn upload_object(
    State(_state): State<StorageState>,
    Path((bucket, path)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<Json<Value>, AppError> {
    tracing::info!("Upload to {}/{} ({} bytes)", bucket, path, body.len());

    // In production, this would stream to S3/MinIO
    Ok(Json(json!({
        "Key": path,
        "bucket": bucket,
        "size": body.len(),
    })))
}

pub async fn get_object(
    State(_state): State<StorageState>,
    Path((bucket, path)): Path<(String, String)>,
) -> Result<Json<Value>, AppError> {
    tracing::info!("Download from {}/{}", bucket, path);

    // In production, this would stream from S3/MinIO
    Ok(Json(json!({
        "Key": path,
        "bucket": bucket,
    })))
}

pub async fn delete_object(
    State(_state): State<StorageState>,
    Path((bucket, path)): Path<(String, String)>,
) -> Result<Json<Value>, AppError> {
    tracing::info!("Delete {}/{}", bucket, path);

    Ok(Json(json!({
        "message": "Object deleted",
    })))
}

pub async fn create_bucket(
    State(_state): State<StorageState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    tracing::info!("Create bucket: {:?}", body);

    Ok(Json(json!({
        "message": "Bucket created",
    })))
}

pub async fn get_bucket(
    State(_state): State<StorageState>,
    Path(bucket): Path<String>,
) -> Result<Json<Value>, AppError> {
    tracing::info!("Get bucket: {}", bucket);

    Ok(Json(json!({
        "id": bucket,
        "name": bucket,
    })))
}
