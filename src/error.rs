use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

/// Machine-readable error codes for IPC and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    #[default]
    Internal,
    ConfigInvalid,
    ConfigPermissionDenied,
    ProfileNotFound,
    ProfileInvalid,
    CoreMissing,
    CoreValidationFailed,
    CoreStartFailed,
    CoreTimeout,
    IpcConnectionFailed,
    IpcTimeout,
    GeoMissing,
    GeoDownloadFailed,
    SystemProxyFailed,
}

impl ErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::ConfigInvalid => "config_invalid",
            Self::ConfigPermissionDenied => "config_permission_denied",
            Self::ProfileNotFound => "profile_not_found",
            Self::ProfileInvalid => "profile_invalid",
            Self::CoreMissing => "core_missing",
            Self::CoreValidationFailed => "core_validation_failed",
            Self::CoreStartFailed => "core_start_failed",
            Self::CoreTimeout => "core_timeout",
            Self::IpcConnectionFailed => "ipc_connection_failed",
            Self::IpcTimeout => "ipc_timeout",
            Self::GeoMissing => "geo_missing",
            Self::GeoDownloadFailed => "geo_download_failed",
            Self::SystemProxyFailed => "system_proxy_failed",
        }
    }
}

/// Structured IPC error carrying a machine-readable code and full cause details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[error("{message}")]
pub struct IpcErrorDetail {
    pub code: ErrorCode,
    pub message: String,
}

/// Core domain errors across clashlime.
#[derive(Debug, Error)]
pub enum ClashlimeError {
    #[error("configuration error at {path}: {message}")]
    Config {
        path: PathBuf,
        message: String,
        code: ErrorCode,
    },

    #[error("core error: {message}")]
    Core {
        message: String,
        code: ErrorCode,
    },

    #[error("geo database error ({name}): {message}")]
    Geo {
        name: String,
        message: String,
        code: ErrorCode,
    },

    #[error("IPC communication error: {message}")]
    Ipc {
        code: ErrorCode,
        message: String,
    },
}

impl ClashlimeError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Config { code, .. }
            | Self::Core { code, .. }
            | Self::Geo { code, .. }
            | Self::Ipc { code, .. } => *code,
        }
    }
}
