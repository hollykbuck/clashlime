/// Sub-pages of the Settings tab. Rows that used to live in one flat
/// 14-row list are grouped by configuration domain; each section keeps
/// its own cursor.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SettingSection {
    #[default]
    Core,
    Network,
    Ports,
    Dns,
    Geo,
    Tun,
}

impl SettingSection {
    pub const ALL: [Self; 6] = [
        Self::Core,
        Self::Network,
        Self::Ports,
        Self::Dns,
        Self::Geo,
        Self::Tun,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Core => "Core",
            Self::Network => "Network",
            Self::Ports => "Ports",
            Self::Dns => "DNS",
            Self::Geo => "Geo",
            Self::Tun => "TUN",
        }
    }

    pub fn row_count(self) -> usize {
        match self {
            Self::Core => 9,
            Self::Network => 13,
            Self::Ports => 8,
            Self::Dns => 15,
            Self::Geo => 9,
            Self::Tun => 10,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Core => 0,
            Self::Network => 1,
            Self::Ports => 2,
            Self::Dns => 3,
            Self::Geo => 4,
            Self::Tun => 5,
        }
    }
}
