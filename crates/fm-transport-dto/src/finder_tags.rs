//! Finder tags and the Spotlight "Finder comment" extended attribute (task 0136).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One of Finder's seven label colors, or no color. Mirrors
/// `fm_platform::FinderTagColor` one-to-one; kept as a separate wire type
/// (specification §3 rule 5: DTOs are never reused as internal domain
/// models).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum FinderTagColorDto {
    /// No label color.
    None,
    /// Finder's built-in "Gray" label color.
    Gray,
    /// Finder's built-in "Green" label color.
    Green,
    /// Finder's built-in "Purple" label color.
    Purple,
    /// Finder's built-in "Blue" label color.
    Blue,
    /// Finder's built-in "Yellow" label color.
    Yellow,
    /// Finder's built-in "Red" label color.
    Red,
    /// Finder's built-in "Orange" label color.
    Orange,
}

/// A single Finder tag: a name, and an optional label color.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FinderTagDto {
    /// The tag's display name, e.g. `"Work"`.
    pub name: String,
    /// The tag's label color.
    pub color: FinderTagColorDto,
}

/// An entry's complete set of Finder tags - used for both reading the
/// current set and replacing it (mirrors [`crate::SettingsDto`]'s get/put
/// symmetry). Replacing matches Finder's own all-at-once tag editor
/// semantics: an empty `tags` removes every tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FinderTagsDto {
    /// The entry's tags, in Finder's own display order.
    pub tags: Vec<FinderTagDto>,
}

/// An entry's Spotlight comment (Get Info's "Comments:" field) - used for
/// both reading the current comment and replacing it. `None` means no
/// comment is set; setting `None` clears an existing comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpotlightCommentDto {
    /// The comment text, or `None` if unset.
    pub comment: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finder_tags_dto_serializes_camel_case_with_a_colored_and_an_uncolored_tag() {
        let dto = FinderTagsDto {
            tags: vec![
                FinderTagDto {
                    name: "Work".to_owned(),
                    color: FinderTagColorDto::Blue,
                },
                FinderTagDto {
                    name: "Plain".to_owned(),
                    color: FinderTagColorDto::None,
                },
            ],
        };

        let value = serde_json::to_value(&dto).expect("serializable DTO");
        assert_eq!(value["tags"][0]["name"], "Work");
        assert_eq!(value["tags"][0]["color"], "blue");
        assert_eq!(value["tags"][1]["color"], "none");

        let round_trip: FinderTagsDto = serde_json::from_value(value).expect("deserializable");
        assert_eq!(round_trip, dto);
    }

    #[test]
    fn an_empty_tag_list_round_trips() {
        let dto = FinderTagsDto { tags: Vec::new() };

        let value = serde_json::to_value(&dto).expect("serializable DTO");
        let round_trip: FinderTagsDto = serde_json::from_value(value).expect("deserializable");

        assert_eq!(round_trip, dto);
    }

    #[test]
    fn spotlight_comment_dto_round_trips_both_a_present_and_an_absent_comment() {
        let present = SpotlightCommentDto {
            comment: Some("Reviewed 2026-08-17".to_owned()),
        };
        let absent = SpotlightCommentDto { comment: None };

        for dto in [present, absent] {
            let value = serde_json::to_value(&dto).expect("serializable DTO");
            let round_trip: SpotlightCommentDto =
                serde_json::from_value(value).expect("deserializable");
            assert_eq!(round_trip, dto);
        }
    }
}
