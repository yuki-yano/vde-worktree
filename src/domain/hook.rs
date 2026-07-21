use std::fmt;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HookName(String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidHookName {
    value: String,
}

impl HookName {
    pub fn pre(action: &str) -> Result<Self, InvalidHookName> {
        Self::from_str(&format!("pre-{action}"))
    }

    pub fn post(action: &str) -> Result<Self, InvalidHookName> {
        Self::from_str(&format!("post-{action}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for HookName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for HookName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for HookName {
    type Err = InvalidHookName;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let suffix = value
            .strip_prefix("pre-")
            .or_else(|| value.strip_prefix("post-"));
        let valid = suffix.is_some_and(|suffix| {
            let mut bytes = suffix.bytes();
            bytes
                .next()
                .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
                && bytes
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        });
        if valid {
            Ok(Self(value.to_owned()))
        } else {
            Err(InvalidHookName {
                value: value.to_owned(),
            })
        }
    }
}

impl fmt::Display for InvalidHookName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid hook name {:?}; expected pre-* or post-* using lowercase ASCII letters, digits, and hyphens",
            self.value
        )
    }
}

impl std::error::Error for InvalidHookName {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_typescript_compatible_hook_names() {
        for value in ["pre-switch", "post-get", "pre-a1", "post-a-b-2"] {
            assert_eq!(value.parse::<HookName>().unwrap().as_str(), value);
        }
    }

    #[test]
    fn rejects_names_that_can_escape_or_do_not_match_the_contract() {
        for value in [
            "switch",
            "pre-",
            "pre--switch",
            "pre-Switch",
            "pre-../switch",
            "post-a/b",
            "post-a_b",
            "pre-é",
        ] {
            assert!(value.parse::<HookName>().is_err(), "accepted {value:?}");
        }
    }
}
