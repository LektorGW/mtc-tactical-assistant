use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MapNameEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MapNameStatus {
    Matched,
    Unknown,
    Ambiguous,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapNameMatch {
    pub status: MapNameStatus,
    pub map_id: Option<String>,
    pub candidates: Vec<String>,
    pub normalized_text: String,
}

/// Normalizes case, whitespace/punctuation and common Polish diacritics; never fuzzy matches.
pub fn normalize_map_name(text: &str) -> String {
    let mut result = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'ą' => 'a',
            'ć' => 'c',
            'ę' => 'e',
            'ł' => 'l',
            'ń' => 'n',
            'ó' => 'o',
            'ś' => 's',
            'ź' | 'ż' => 'z',
            other => other,
        };
        if c.is_alphanumeric() {
            result.push(c);
        } else if !result.is_empty() && !result.ends_with(' ') {
            result.push(' ');
        }
    }
    result.trim_end().to_owned()
}

/// Only full normalized canonical names, explicit aliases and IDs may produce a catalog identity.
pub fn match_map_name(text: &str, catalog: &[MapNameEntry]) -> Result<MapNameMatch, String> {
    if catalog.len() > 1024 || text.len() > 4096 {
        return Err("Nazwa lub katalog map przekracza limit".into());
    }
    let mut ids = BTreeSet::new();
    for entry in catalog {
        if entry.id.trim().is_empty()
            || entry.name.trim().is_empty()
            || !ids.insert(&entry.id)
            || entry.aliases.len() > 64
            || entry.aliases.iter().any(|a| a.len() > 256)
        {
            return Err("Niepoprawne lub powtórzone wpisy katalogu map".into());
        }
    }
    let normalized_text = normalize_map_name(text);
    let candidates: Vec<_> = catalog
        .iter()
        .filter(|entry| {
            !normalized_text.is_empty()
                && std::iter::once(entry.name.as_str())
                    .chain(std::iter::once(entry.id.as_str()))
                    .chain(entry.aliases.iter().map(String::as_str))
                    .any(|name| normalize_map_name(name) == normalized_text)
        })
        .map(|entry| entry.id.clone())
        .collect();
    let status = match candidates.len() {
        0 => MapNameStatus::Unknown,
        1 => MapNameStatus::Matched,
        _ => MapNameStatus::Ambiguous,
    };
    Ok(MapNameMatch {
        status,
        map_id: (status == MapNameStatus::Matched).then(|| candidates[0].clone()),
        candidates,
        normalized_text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(id: &str, name: &str, aliases: &[&str]) -> MapNameEntry {
        MapNameEntry {
            id: id.into(),
            name: name.into(),
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
        }
    }
    #[test]
    fn uses_exact_alias_without_guessing_a_partial_or_unknown_name() {
        let catalog = vec![
            entry("arctic_airbase", "Arctic Airbase", &["Airbase"]),
            entry("chernobyl_v2", "Chernobyl", &[]),
        ];
        assert_eq!(
            match_map_name(" ARCTIC--AIRBASE ", &catalog)
                .unwrap()
                .map_id
                .as_deref(),
            Some("arctic_airbase")
        );
        assert_eq!(
            match_map_name("Airbase", &catalog)
                .unwrap()
                .map_id
                .as_deref(),
            Some("arctic_airbase")
        );
        assert_eq!(
            match_map_name("Arctic", &catalog).unwrap().status,
            MapNameStatus::Unknown
        );
        assert_eq!(
            match_map_name("Unknown map", &catalog).unwrap().status,
            MapNameStatus::Unknown
        );
        assert_eq!(
            match_map_name("", &catalog).unwrap().status,
            MapNameStatus::Unknown
        );
    }
    #[test]
    fn ambiguous_alias_does_not_choose_the_first_map() {
        let result = match_map_name(
            "Military Base",
            &[
                entry("a", "A", &["Military Base"]),
                entry("b", "B", &["Military Base"]),
            ],
        )
        .unwrap();
        assert_eq!(result.status, MapNameStatus::Ambiguous);
        assert!(result.map_id.is_none());
        assert_eq!(result.candidates.len(), 2);
    }
    #[test]
    fn normalizes_polish_letters_and_rejects_duplicate_ids() {
        assert_eq!(normalize_map_name(" ŁĄKA  PÓŁNOCNA "), "laka polnocna");
        assert!(match_map_name("A", &[entry("same", "A", &[]), entry("same", "B", &[])]).is_err());
    }
}
