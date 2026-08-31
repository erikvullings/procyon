//! Wire representation of [`fm_domain::Location`] (spec §5.1, §8).

use fm_domain::{Location, ProviderId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A provider-neutral pointer to a location, for example a directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({"providerId": "local", "uri": "file:///Users/erik/Documents"}))]
pub struct LocationDto {
    /// The virtual filesystem provider that owns this location.
    pub provider_id: String,
    /// The full, provider-specific URI text.
    pub uri: String,
}

impl From<Location> for LocationDto {
    fn from(location: Location) -> Self {
        Self {
            provider_id: location.provider_id.as_str().to_owned(),
            uri: location.uri,
        }
    }
}

impl From<LocationDto> for Location {
    fn from(dto: LocationDto) -> Self {
        Location::new(ProviderId::new(dto.provider_id), dto.uri)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LocationDto {
        LocationDto {
            provider_id: "local".to_owned(),
            uri: "file:///Users/erik/Documents".to_owned(),
        }
    }

    #[test]
    fn location_dto_round_trips_through_serde_json() {
        let dto = sample();
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        let parsed: LocationDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(dto, parsed);
    }

    #[test]
    fn location_dto_uses_camel_case_field_names() {
        let json = serde_json::to_string(&sample()).expect("serialization must succeed");
        assert!(json.contains("\"providerId\""));
        assert!(json.contains("\"uri\""));
    }

    #[test]
    fn location_dto_converts_to_and_from_the_domain_type() {
        let location = Location::new(ProviderId::new("local"), "file:///Users/erik/Documents");
        let dto: LocationDto = location.clone().into();
        let round_tripped: Location = dto.into();
        assert_eq!(location, round_tripped);
    }
}
