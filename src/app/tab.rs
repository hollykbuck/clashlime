use crossterm::event::KeyCode;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Dashboard,
    Proxies,
    Profiles,
    Connections,
    Rules,
    Logs,
    Settings,
    Help,
}

impl Tab {
    pub const ALL: [Self; 8] = [
        Self::Dashboard,
        Self::Proxies,
        Self::Profiles,
        Self::Connections,
        Self::Rules,
        Self::Logs,
        Self::Settings,
        Self::Help,
    ];
    pub(crate) fn shortcut(code: &KeyCode) -> Option<Self> {
        match code {
            KeyCode::Char('1') => Some(Self::Dashboard),
            KeyCode::Char('2') => Some(Self::Proxies),
            KeyCode::Char('3') => Some(Self::Profiles),
            KeyCode::Char('4') => Some(Self::Connections),
            KeyCode::Char('5') => Some(Self::Rules),
            KeyCode::Char('6') => Some(Self::Logs),
            KeyCode::Char('7') => Some(Self::Settings),
            KeyCode::Char('8') => Some(Self::Help),
            _ => None,
        }
    }
}
