use crate::domain::error::{CliError, ErrorCode};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommonSafetyPolicy {
    pub hooks_disabled: bool,
    pub allow_unsafe: bool,
}

pub fn enforce_common_safety(policy: CommonSafetyPolicy) -> Result<(), CliError> {
    if policy.hooks_disabled && !policy.allow_unsafe {
        return Err(CliError::new(
            ErrorCode::UnsafeFlagRequired,
            "--no-hooks requires --allow-unsafe",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabling_hooks_requires_explicit_unsafe_consent() {
        let error = enforce_common_safety(CommonSafetyPolicy {
            hooks_disabled: true,
            allow_unsafe: false,
        })
        .expect_err("disabled hooks must be rejected");

        assert_eq!(error.code, ErrorCode::UnsafeFlagRequired);
        assert_eq!(error.exit_code(), 4);
        assert!(
            enforce_common_safety(CommonSafetyPolicy {
                hooks_disabled: true,
                allow_unsafe: true,
            })
            .is_ok()
        );
    }
}
