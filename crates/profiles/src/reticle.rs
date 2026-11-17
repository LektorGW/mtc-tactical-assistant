use serde::{Deserialize, Serialize};

/// One sight graduation: aiming with `mark` is assumed to compensate the drop
/// at `range` (profile unit). A player-edited assumption, never verified data.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReticleMark {
    pub mark: String,
    pub range: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReticleMap {
    pub marks: Vec<ReticleMark>,
    /// `unverified` (assumed mapping), `experimental` or `beta` (checked by shots).
    pub status: String,
    pub source: String,
}

impl ReticleMap {
    pub fn validate(&self) -> Result<(), String> {
        if self.marks.is_empty()
            || self.marks.len() > 32
            || !["unverified", "experimental", "beta"].contains(&self.status.as_str())
            || self.source.trim().is_empty()
            || self.source.len() > 512
        {
            return Err("Mapa kresek celownika wymaga 1–32 kresek, statusu i źródła".into());
        }
        let mut previous = 0.0;
        for mark in &self.marks {
            if mark.mark.trim().is_empty()
                || mark.mark.chars().count() > 16
                || !mark.range.is_finite()
                || mark.range <= previous
                || mark.range > 1e5
            {
                return Err(
                    "Kreski celownika: niepuste etykiety i rosnące dodatnie dystanse".into(),
                );
            }
            previous = mark.range;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pgo7() -> ReticleMap {
        ReticleMap {
            marks: [("2", 200.0), ("3", 300.0), ("4", 400.0), ("5", 500.0)]
                .iter()
                .map(|(mark, range)| ReticleMark {
                    mark: (*mark).into(),
                    range: *range,
                })
                .collect(),
            status: "unverified".into(),
            source: "Założenie gracza: kreska N = N×100 m (PGO-7)".into(),
        }
    }

    #[test]
    fn accepts_assumed_increasing_graduations() {
        pgo7().validate().unwrap();
    }

    #[test]
    fn rejects_unsorted_empty_and_unsourced_marks() {
        let mut r = pgo7();
        r.marks.swap(0, 1);
        assert!(r.validate().is_err());
        let mut r = pgo7();
        r.source.clear();
        assert!(r.validate().is_err());
        let mut r = pgo7();
        r.marks.clear();
        assert!(r.validate().is_err());
        let mut r = pgo7();
        r.status = "verified".into();
        assert!(r.validate().is_err());
    }
}
