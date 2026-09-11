/// Sub-pages of the Settings tab. Rows that used to live in one flat
/// 14-row list are grouped by configuration domain; each section keeps
/// its own cursor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingSection {
    Core,
    Network,
    Dns,
    Geo,
}

impl SettingSection {
    pub const ALL: [Self; 4] = [Self::Core, Self::Network, Self::Dns, Self::Geo];

    pub fn title(self) -> &'static str {
        match self {
            Self::Core => "Core",
            Self::Network => "Network",
            Self::Dns => "DNS",
            Self::Geo => "Geo",
        }
    }

    pub fn row_count(self) -> usize {
        match self {
            Self::Core => 3,
            Self::Network => 3,
            Self::Dns => 3,
            Self::Geo => 5,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Core => 0,
            Self::Network => 1,
            Self::Dns => 2,
            Self::Geo => 3,
        }
    }
}
