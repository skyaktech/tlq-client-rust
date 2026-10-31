use std::time::Duration;

/// Server metadata returned by [`TlqClient::server_info`](crate::TlqClient::server_info).
///
/// TLQ 0.5.0 includes its version and uptime in the `/hello` JSON string.
/// Older servers return `"Hello World"`, leaving both metadata fields `None`.
/// Unrecognized greetings are preserved without failing the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    /// Original greeting, decoded from the server's JSON response.
    pub greeting: String,
    /// Server version, without the `v` prefix, when recognized.
    pub version: Option<String>,
    /// Server uptime, with second precision, when recognized.
    pub uptime: Option<Duration>,
}

impl ServerInfo {
    pub(crate) fn from_greeting(greeting: String) -> Self {
        let mut info = Self {
            greeting,
            version: None,
            uptime: None,
        };

        if let Some((version, uptime)) = info
            .greeting
            .strip_prefix("Hello TLQ v")
            .and_then(|value| value.strip_suffix(')'))
            .and_then(|value| value.split_once(" (uptime: "))
        {
            if !version.is_empty() && !version.chars().any(char::is_whitespace) {
                info.version = Some(version.to_string());
                info.uptime = parse_uptime(uptime);
            }
        }

        info
    }
}

fn parse_uptime(value: &str) -> Option<Duration> {
    let mut seconds = 0_u64;
    let mut previous_multiplier = u64::MAX;

    for component in value.split_whitespace() {
        let unit = component.chars().last()?;
        let multiplier = match unit {
            'd' => 86_400,
            'h' => 3_600,
            'm' => 60,
            's' => 1,
            _ => return None,
        };
        let digits = component.strip_suffix(unit)?;
        if multiplier >= previous_multiplier || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let amount = digits.parse::<u64>().ok()?;
        seconds = seconds.checked_add(amount.checked_mul(multiplier)?)?;
        previous_multiplier = multiplier;
    }

    (previous_multiplier != u64::MAX).then_some(Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_server_uptime_formats() {
        for (uptime, seconds) in [
            ("0s", 0),
            ("59s", 59),
            ("1m 5s", 65),
            ("1h 0m 0s", 3_600),
            ("1d 0h 0m 0s", 86_400),
            ("1d 2h 3m 4s", 93_784),
        ] {
            let greeting = format!("Hello TLQ v0.5.0-rc.1 (uptime: {uptime})");
            let info = ServerInfo::from_greeting(greeting.clone());
            assert_eq!(info.greeting, greeting);
            assert_eq!(info.version.as_deref(), Some("0.5.0-rc.1"));
            assert_eq!(info.uptime, Some(Duration::from_secs(seconds)));
        }
    }

    #[test]
    fn preserves_legacy_and_unrecognized_greetings() {
        for greeting in [
            "Hello World",
            "A future greeting format",
            "Hello TLQ v (uptime: 5s)",
            "Hello TLQ v0.5.0 (uptime: 5s",
        ] {
            let info = ServerInfo::from_greeting(greeting.to_string());
            assert_eq!(info.greeting, greeting);
            assert_eq!(info.version, None);
            assert_eq!(info.uptime, None);
        }
    }

    #[test]
    fn invalid_uptime_does_not_discard_greeting_or_version() {
        for uptime in [
            "",
            "s",
            "-1s",
            "+1s",
            "1.5s",
            "1w",
            "1s 2s",
            "1s 1m",
            "∞s",
            "18446744073709551616s",
            "18446744073709551615d",
            "1m 18446744073709551615s",
        ] {
            let greeting = format!("Hello TLQ v0.5.0 (uptime: {uptime})");
            let info = ServerInfo::from_greeting(greeting.clone());
            assert_eq!(info.greeting, greeting);
            assert_eq!(info.version.as_deref(), Some("0.5.0"));
            assert_eq!(info.uptime, None, "unexpected uptime: {uptime}");
        }
    }
}
