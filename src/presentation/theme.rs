use anstyle::{Color, RgbColor, Style};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorPolicy {
    pub stream_is_terminal: bool,
    pub json: bool,
    pub no_color: bool,
}

impl ColorPolicy {
    pub const fn enabled(self) -> bool {
        self.stream_is_terminal && !self.json && !self.no_color
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticStyle {
    Header,
    BranchCurrent,
    Branch,
    BranchDetached,
    Dirty,
    Safe,
    Attention,
    Unknown,
    Base,
    Path,
    Muted,
    Value,
    PreviewLabel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CatppuccinMocha;

impl CatppuccinMocha {
    pub const ROSEWATER: RgbColor = RgbColor(0xf5, 0xe0, 0xdc);
    pub const MAUVE: RgbColor = RgbColor(0xcb, 0xa6, 0xf7);
    pub const RED: RgbColor = RgbColor(0xf3, 0x8b, 0xa8);
    pub const PEACH: RgbColor = RgbColor(0xfa, 0xb3, 0x87);
    pub const YELLOW: RgbColor = RgbColor(0xf9, 0xe2, 0xaf);
    pub const GREEN: RgbColor = RgbColor(0xa6, 0xe3, 0xa1);
    pub const BLUE: RgbColor = RgbColor(0x89, 0xb4, 0xfa);
    pub const LAVENDER: RgbColor = RgbColor(0xb4, 0xbe, 0xfe);
    pub const SAPPHIRE: RgbColor = RgbColor(0x74, 0xc7, 0xec);
    pub const TEXT: RgbColor = RgbColor(0xcd, 0xd6, 0xf4);
    pub const OVERLAY0: RgbColor = RgbColor(0x6c, 0x70, 0x86);

    pub const fn rgb(style: SemanticStyle) -> RgbColor {
        match style {
            SemanticStyle::Header => Self::ROSEWATER,
            SemanticStyle::BranchCurrent | SemanticStyle::PreviewLabel => Self::MAUVE,
            SemanticStyle::Branch => Self::LAVENDER,
            SemanticStyle::BranchDetached | SemanticStyle::Dirty => Self::PEACH,
            SemanticStyle::Safe => Self::GREEN,
            SemanticStyle::Attention => Self::RED,
            SemanticStyle::Unknown => Self::YELLOW,
            SemanticStyle::Base => Self::BLUE,
            SemanticStyle::Path => Self::SAPPHIRE,
            SemanticStyle::Muted => Self::OVERLAY0,
            SemanticStyle::Value => Self::TEXT,
        }
    }

    pub fn paint(style: SemanticStyle, value: &str, policy: ColorPolicy) -> String {
        if !policy.enabled() || value.is_empty() {
            return value.to_owned();
        }
        let ansi = Style::new().fg_color(Some(Color::Rgb(Self::rgb(style))));
        format!("{ansi}{value}{ansi:#}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_exact_catppuccin_mocha_truecolor() {
        let policy = ColorPolicy {
            stream_is_terminal: true,
            json: false,
            no_color: false,
        };
        assert_eq!(
            CatppuccinMocha::paint(SemanticStyle::Header, "branch", policy),
            "\u{1b}[38;2;245;224;220mbranch\u{1b}[0m"
        );
        assert_eq!(
            CatppuccinMocha::paint(SemanticStyle::BranchCurrent, "* main", policy),
            "\u{1b}[38;2;203;166;247m* main\u{1b}[0m"
        );
        assert_eq!(
            CatppuccinMocha::paint(SemanticStyle::Path, "~/repo", policy),
            "\u{1b}[38;2;116;199;236m~/repo\u{1b}[0m"
        );
    }

    #[test]
    fn never_emits_ansi_for_non_tty_json_or_no_color() {
        for policy in [
            ColorPolicy {
                stream_is_terminal: false,
                json: false,
                no_color: false,
            },
            ColorPolicy {
                stream_is_terminal: true,
                json: true,
                no_color: false,
            },
            ColorPolicy {
                stream_is_terminal: true,
                json: false,
                no_color: true,
            },
        ] {
            let output = CatppuccinMocha::paint(SemanticStyle::Attention, "dirty", policy);
            assert_eq!(output, "dirty");
            assert!(!output.contains('\u{1b}'));
        }
    }
}
