//! Health check response (task 0008, `GET /api/v1/health`).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Health status of the backend process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum HealthStatusDto {
    /// The backend is running and able to serve requests.
    Ok,
}

/// Response body for `GET /api/v1/health`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({"status": "ok"}))]
pub struct HealthDto {
    /// Current health status.
    pub status: HealthStatusDto,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_dto_round_trips_through_serde_json() {
        let dto = HealthDto {
            status: HealthStatusDto::Ok,
        };
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        let parsed: HealthDto = serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(dto, parsed);
    }

    #[test]
    fn health_dto_matches_the_documented_shape() {
        let dto = HealthDto {
            status: HealthStatusDto::Ok,
        };
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        assert_eq!(json, "{\"status\":\"ok\"}");
    }
}
