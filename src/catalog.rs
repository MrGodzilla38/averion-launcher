//! Offline release previews. Entries carry no installation or entitlement state.
pub struct Version {
    pub id: &'static str,
    pub title: &'static str,
    pub cover: &'static str,
}

macro_rules! versions {
    ($($cover:literal, $title:literal => [$($id:literal),+]),+ $(,)?) => {
        &[$($(Version { id: $id, title: $title, cover: $cover },)+)+]
    };
}

// Keep stable releases newest first. The first entry is the startup default.
pub const VERSIONS: &[Version] = versions![
    "26.3", "Wilderness Bound" => ["26.3"],
    "26.2", "Chaos Cubed" => ["26.2"],
    "26.1", "Tiny Takeover" => ["26.1.2", "26.1.1", "26.1"],
    "1.21", "Tricky Trials" => ["1.21.11", "1.21.10", "1.21.9", "1.21.8", "1.21.7", "1.21.6", "1.21.5", "1.21.4", "1.21.3", "1.21.2", "1.21.1", "1.21"],
    "1.20", "Trails & Tales" => ["1.20.6", "1.20.5", "1.20.4", "1.20.3", "1.20.2", "1.20.1", "1.20"],
    "1.19", "The Wild Update" => ["1.19.4", "1.19.3", "1.19.2", "1.19.1", "1.19"],
    "1.18", "Caves & Cliffs: Part II" => ["1.18.2", "1.18.1", "1.18"],
    "1.17", "Caves & Cliffs: Part I" => ["1.17.1", "1.17"],
    "1.16", "Nether Update" => ["1.16.5"],
];

pub fn families() -> impl Iterator<Item = &'static Version> {
    VERSIONS.iter().enumerate().filter_map(|(index, version)| {
        (index == 0 || VERSIONS[index - 1].cover != version.cover).then_some(version)
    })
}

pub fn patches(family: &'static str) -> impl Iterator<Item = (usize, &'static Version)> {
    VERSIONS
        .iter()
        .enumerate()
        .filter(move |(_, version)| version.cover == family)
}
