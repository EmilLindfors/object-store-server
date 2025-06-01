use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use bytes::Bytes;

use crate::{
    adapters::inbound::http::{
        dto::{
            ErrorResponseDto, ListObjectsDto, ListObjectsResponseDto, ObjectInfoDto,
            SuccessResponseDto,
        },
        router::AppState,
    },
    domain::{
        models::{CreateObjectRequest, GetObjectRequest},
        value_objects::ObjectKey,
    },
    ports::storage::ObjectInfo,
};

/// Handle object creation
pub async fn create_object(
    State(app_state): State<AppState>,
    Path(key): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<SuccessResponseDto>), (StatusCode, Json<ErrorResponseDto>)> {
    let object_service = &app_state.object_service;

    // Extract content type from headers
    let content_type = headers
        .get("content-type")
        .and_then(|ct| ct.to_str().ok())
        .map(|s| s.to_string());

    // Create object key
    let object_key = ObjectKey::new(key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid object key: {}",
                e
            ))),
        )
    })?;

    // Create request
    let request = CreateObjectRequest {
        key: object_key,
        data: body.to_vec(),
        content_type,
        custom_metadata: Default::default(),
    };

    // Store the object
    object_service.create_object(request).await.map_err(|e| {
        let status_code = StatusCode::from(e.clone());
        (status_code, Json(ErrorResponseDto::from_storage_error(e)))
    })?;

    Ok((
        StatusCode::CREATED,
        Json(SuccessResponseDto::new("Object created successfully")),
    ))
}

/// Handle object retrieval
pub async fn get_object(
    State(app_state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Response<Body>, (StatusCode, Json<ErrorResponseDto>)> {
    let object_service = &app_state.object_service;

    // Create object key
    let object_key = ObjectKey::new(key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid object key: {}",
                e
            ))),
        )
    })?;

    // Create request
    let request = GetObjectRequest {
        key: object_key,
        version_id: None,
    };

    // Get the object
    let storage_object = object_service.get_object(request).await.map_err(|e| {
        let status_code = StatusCode::from(e.clone());
        (status_code, Json(ErrorResponseDto::from_storage_error(e)))
    })?;

    // Return the object data
    let content_type = storage_object
        .metadata
        .content_type
        .as_deref()
        .unwrap_or("application/octet-stream");

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .body(Body::from(storage_object.data))
        .unwrap())
}

/// Handle object deletion
pub async fn delete_object(
    State(app_state): State<AppState>,
    Path(key): Path<String>,
) -> Result<(StatusCode, Json<SuccessResponseDto>), (StatusCode, Json<ErrorResponseDto>)> {
    let object_service = &app_state.object_service;

    // Create object key
    let object_key = ObjectKey::new(key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid object key: {}",
                e
            ))),
        )
    })?;

    // Delete the object
    object_service
        .delete_object(&object_key)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_storage_error(e)))
        })?;

    Ok((
        StatusCode::OK,
        Json(SuccessResponseDto::new("Object deleted successfully")),
    ))
}

/// Handle object existence check
pub async fn head_object(
    State(app_state): State<AppState>,
    Path(key): Path<String>,
) -> Result<(StatusCode, HeaderMap), (StatusCode, Json<ErrorResponseDto>)> {
    let object_service = &app_state.object_service;

    // Create object key
    let object_key = ObjectKey::new(key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid object key: {}",
                e
            ))),
        )
    })?;

    // Check if object exists and get its size
    let exists = object_service
        .object_exists(&object_key)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_storage_error(e)))
        })?;

    if !exists {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponseDto::bad_request("Object not found")),
        ));
    }

    let size = object_service
        .get_object_size(&object_key)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_storage_error(e)))
        })?;

    let mut headers = HeaderMap::new();
    headers.insert("content-length", size.to_string().parse().unwrap());
    headers.insert("content-type", "application/octet-stream".parse().unwrap());

    Ok((StatusCode::OK, headers))
}

/// Handle object listing
pub async fn list_objects(
    State(app_state): State<AppState>,
    Query(params): Query<ListObjectsDto>,
) -> Result<Json<ListObjectsResponseDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let object_service = &app_state.object_service;

    // List objects with optional prefix and max results
    let objects = object_service
        .list_objects(params.prefix.as_deref(), params.max_results)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_storage_error(e)))
        })?;

    // Convert to DTOs
    let object_dtos: Vec<ObjectInfoDto> = objects
        .into_iter()
        .map(|obj| ObjectInfoDto {
            key: obj.key.as_str().to_string(),
            size: obj.size,
            last_modified: obj.last_modified.into(),
            etag: obj.etag,
            storage_class: None, // Would need to be provided by the storage layer
            version_id: None,    // Would be set for versioned objects
        })
        .collect();

    let total_count = object_dtos.len();
    let is_truncated = params.max_results.map_or(false, |max| total_count >= max);

    Ok(Json(ListObjectsResponseDto {
        objects: object_dtos,
        is_truncated,
        next_continuation_token: None, // Would be implemented for pagination
        total_count,
    }))
}

/// Handle object copy
pub async fn copy_object(
    State(app_state): State<AppState>,
    Path((source_key, dest_key)): Path<(String, String)>,
) -> Result<(StatusCode, Json<SuccessResponseDto>), (StatusCode, Json<ErrorResponseDto>)> {
    let object_service = &app_state.object_service;

    // Create object keys
    let source_object_key = ObjectKey::new(source_key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid source key: {}",
                e
            ))),
        )
    })?;

    let dest_object_key = ObjectKey::new(dest_key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid destination key: {}",
                e
            ))),
        )
    })?;

    // Copy the object
    object_service
        .copy_object(&source_object_key, &dest_object_key)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_storage_error(e)))
        })?;

    Ok((
        StatusCode::OK,
        Json(SuccessResponseDto::new("Object copied successfully")),
    ))
}

/// Convert ObjectInfo to ObjectInfoDto helper
impl From<ObjectInfo> for ObjectInfoDto {
    fn from(info: ObjectInfo) -> Self {
        ObjectInfoDto {
            key: info.key.as_str().to_string(),
            size: info.size,
            last_modified: info.last_modified.into(),
            etag: info.etag,
            storage_class: None,
            version_id: None,
        }
    }
}
