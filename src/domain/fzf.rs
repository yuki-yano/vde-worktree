use std::fmt;

const RESERVED_OPTIONS: [&str; 5] = ["prompt", "layout", "height", "border", "tmux"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FzfArgsError {
    message: String,
}

impl FzfArgsError {
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for FzfArgsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for FzfArgsError {}

pub fn validate_fzf_extra_args<I, S>(args: I) -> Result<(), FzfArgsError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    for argument in args {
        let argument = argument.as_ref();
        if argument.is_empty() {
            return Err(FzfArgsError {
                message: "empty value is not allowed for --fzf-arg".to_owned(),
            });
        }
        let Some(long_option) = argument.strip_prefix("--") else {
            continue;
        };
        let option_name = long_option
            .split_once('=')
            .map_or(long_option, |(name, _)| name);
        let normalized_option_name = option_name.strip_prefix("no-").unwrap_or(option_name);
        if RESERVED_OPTIONS.contains(&normalized_option_name) {
            return Err(FzfArgsError {
                message: format!(
                    "--fzf-arg cannot override reserved fzf option: --{normalized_option_name}"
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_reserved_options_in_both_flag_forms() {
        for argument in [
            "--prompt=hack> ",
            "--layout",
            "--height=100%",
            "--border=none",
            "--tmux=100%,100%",
            "--no-height",
            "--no-border",
            "--no-tmux",
        ] {
            let error = validate_fzf_extra_args([argument]).expect_err("reserved option");
            assert!(error.to_string().contains("reserved fzf option"));
        }
    }

    #[test]
    fn accepts_non_reserved_options_and_option_like_values() {
        validate_fzf_extra_args(["--ansi", "--nth=1", "-x", "query"])
            .expect("non-reserved options are accepted");
    }
}
