use fm_domain::{Location, location::validate_name};

use crate::VfsError;

/// Validates the common URI shape used by connection-backed providers.
///
/// The provider still owns the decision to use this grammar and which schemes
/// it accepts; this helper keeps identical safety rules from drifting.
pub fn validate_connection_location(
    location: &Location,
    provider_id: &str,
    schemes: &[&str],
    reject_windows_names: bool,
) -> Result<(), VfsError> {
    let scheme = location.scheme().map_err(|_| invalid_location(location))?;
    if location.provider_id.as_str() != provider_id || !schemes.contains(&scheme) {
        return Err(invalid_location(location));
    }
    let (_, remainder) = location
        .uri
        .split_once("://")
        .ok_or_else(|| invalid_location(location))?;
    if remainder.contains(['?', '#']) {
        return Err(invalid_location(location));
    }
    let (connection_id, path) = remainder
        .split_once('/')
        .ok_or_else(|| invalid_location(location))?;
    uuid::Uuid::parse_str(connection_id).map_err(|_| invalid_location(location))?;
    if path.is_empty() {
        return Ok(());
    }
    for encoded in path.split('/') {
        if encoded.is_empty() {
            return Err(invalid_location(location));
        }
        let decoded = percent_decode(encoded).ok_or_else(|| invalid_location(location))?;
        if decoded.contains(&0) || decoded.contains(&b'/') || decoded.contains(&b'\\') {
            return Err(invalid_location(location));
        }
        if reject_windows_names
            && let Ok(name) = std::str::from_utf8(&decoded)
            && name != "."
            && name != ".."
        {
            validate_name(name).map_err(|_| invalid_location(location))?;
        }
    }
    Ok(())
}

fn percent_decode(segment: &str) -> Option<Vec<u8>> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let high = hex_value(*bytes.get(index + 1)?)?;
        let low = hex_value(*bytes.get(index + 2)?)?;
        decoded.push(high * 16 + low);
        index += 3;
    }
    Some(decoded)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn invalid_location(location: &Location) -> VfsError {
    VfsError::InvalidLocation {
        location: location.uri.clone(),
    }
}

#[cfg(test)]
mod tests {
    use fm_domain::{Location, ProviderId};

    use super::*;

    #[test]
    fn connection_validation_preserves_the_existing_uri_safety_contract() {
        let valid_id = "11111111-1111-4111-8111-111111111111";
        for uri in [
            "sftp://not-a-uuid/home",
            "sftp://11111111-1111-4111-8111-111111111111",
            "sftp://11111111-1111-4111-8111-111111111111//home",
            "sftp://11111111-1111-4111-8111-111111111111/bad%2Fname",
            "sftp://11111111-1111-4111-8111-111111111111/CON.txt",
            "sftp://11111111-1111-4111-8111-111111111111/home?query=1",
        ] {
            let location = Location::new(ProviderId::new("sftp"), uri);
            assert!(
                validate_connection_location(&location, "sftp", &["sftp"], true).is_err(),
                "accepted {uri}"
            );
        }

        let location = Location::new(
            ProviderId::new("sftp"),
            format!("sftp://{valid_id}/home/My%20Documents"),
        );
        validate_connection_location(&location, "sftp", &["sftp"], true)
            .expect("valid connection URI");
    }

    #[test]
    fn connection_validation_can_preserve_ftp_device_names() {
        let location = Location::new(
            ProviderId::new("ftp"),
            "ftp://11111111-1111-4111-8111-111111111111/CON.txt",
        );

        validate_connection_location(&location, "ftp", &["ftp", "ftps"], false)
            .expect("FTP historically permits Windows device names");
    }
}
