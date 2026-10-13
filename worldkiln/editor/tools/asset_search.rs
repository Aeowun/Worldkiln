#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AssetOrigin {
    BuiltIn,
    Project,
}

impl AssetOrigin {
    pub fn label(self) -> &'static str {
        match self {
            Self::BuiltIn => "BUILT-IN",
            Self::Project => "PROJECT",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AssetEntry {
    pub path: String,
    pub folder: String,
    pub origin: AssetOrigin,
}

impl AssetEntry {
    pub fn new(path: impl Into<String>, origin: AssetOrigin) -> Self {
        let path = path.into().replace('\\', "/");
        let folder = asset_folder(&path);

        Self {
            path,
            folder,
            origin,
        }
    }
}

pub(crate) fn asset_folder(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(folder, _)| folder.to_string())
        .filter(|folder| !folder.is_empty())
        .unwrap_or_else(|| "Root".to_string())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AssetBrowserState {
    pub open: bool,
    pub search: String,
    pub target: String,
}

impl AssetBrowserState {
    pub fn open_for(&mut self, target: impl Into<String>) {
        self.open = true;
        self.target = target.into();
    }

    pub fn is_open_for(&self, target: &str) -> bool {
        self.open && self.target == target
    }
}

fn asset_match_score(query: &str, name: &str) -> Option<u32> {
    let query = query.trim().to_lowercase();
    let name = name.to_lowercase();

    if query.is_empty() {
        return None;
    }

    if name == query {
        return Some(0);
    }

    if name.starts_with(&query) {
        return Some(10);
    }

    if name.contains(&query) {
        return Some(20);
    }

    let query_chars: Vec<char> = query.chars().collect();
    let name_chars: Vec<char> = name.chars().collect();

    let mut query_index = 0;

    for character in name_chars {
        if query_index < query_chars.len() && character == query_chars[query_index] {
            query_index += 1;
        }
    }

    (query_index == query_chars.len()).then_some(30)
}

pub fn ranked_asset_matches<I>(names: I, query: &str, limit: usize) -> Vec<String>
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let mut matches: Vec<(u32, String)> = names
        .into_iter()
        .filter_map(|name| {
            let name = name.as_ref();

            asset_match_score(query, name).map(|score| (score, name.to_string()))
        })
        .collect();

    matches.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    matches
        .into_iter()
        .take(limit)
        .map(|(_, name)| name)
        .collect()
}

#[cfg(test)]
mod browser_target_tests {
    use super::{AssetBrowserState, AssetEntry, AssetOrigin, asset_folder};

    #[test]
    fn asset_entry_normalizes_folder_and_origin() {
        let entry = AssetEntry::new(r"ambience\wind\soft.wav", AssetOrigin::Project);

        assert_eq!(entry.path, "ambience/wind/soft.wav");
        assert_eq!(entry.folder, "ambience/wind");
        assert_eq!(entry.origin.label(), "PROJECT");
        assert_eq!(asset_folder("click.wav"), "Root");
    }

    #[test]
    fn browser_request_is_owned_by_the_property_that_opened_it() {
        let mut state = AssetBrowserState::default();
        state.open_for("cell.42.audio");

        assert!(state.is_open_for("cell.42.audio"));
        assert!(!state.is_open_for("build.audio"));
        assert!(!state.is_open_for("cell.99.audio"));
    }

    #[test]
    fn closed_browser_never_matches_a_property() {
        let mut state = AssetBrowserState::default();
        state.open_for("cell.42.texture");
        state.open = false;

        assert!(!state.is_open_for("cell.42.texture"));
    }
}
