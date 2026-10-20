use super::valueobject::{
    Candidate, Catalog, DomainError, GuideSlot, Manifest, ManifestEntry, Plan, SCHEMA_VERSION,
    Segment, SlotPlan,
};
use std::collections::BTreeMap;

pub fn parse_guide(text: &str) -> Vec<GuideSlot> {
    let structured: Vec<(String, String)> = text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (role, query) = line.split_once([':', '：'])?;
            let role = role.trim();
            let query = query.trim();
            (!role.is_empty() && !query.is_empty()).then(|| (role.to_owned(), query.to_owned()))
        })
        .collect();
    let pairs = if structured.is_empty() {
        parse_natural_guide(text)
    } else {
        structured
    };
    pairs
        .into_iter()
        .enumerate()
        .map(|(index, (role, query))| GuideSlot {
            id: format!("slot-{}", index + 1),
            order: index + 1,
            role,
            query,
        })
        .collect()
}

fn parse_natural_guide(text: &str) -> Vec<(String, String)> {
    let mut slots = Vec::new();
    let normalized = text.replace(['、', '。'], "\n");
    for phrase in normalized
        .lines()
        .map(str::trim)
        .filter(|phrase| !phrase.is_empty())
    {
        if let Some((asset, rest)) = phrase.split_once('を')
            && let Some((role, _)) = rest.split_once("に置")
        {
            slots.push((role.trim().to_owned(), asset.trim().to_owned()));
            continue;
        }
        if let Some((qualifier, rest)) = phrase.split_once('の')
            && let Some((role, _)) = rest.split_once('で')
        {
            slots.push((
                role.trim().to_owned(),
                format!("{} {}", qualifier.trim(), role.trim()),
            ));
            continue;
        }
        slots.push(("素材".to_owned(), phrase.to_owned()));
    }
    slots
}

pub fn create_plan(catalog: &Catalog, guide_text: &str) -> Plan {
    Plan {
        schema_version: SCHEMA_VERSION,
        guide_text: guide_text.to_owned(),
        material_root: catalog.material_root.clone(),
        slots: parse_guide(guide_text)
            .into_iter()
            .map(|slot| {
                let mut candidates: Vec<_> = catalog
                    .segments
                    .iter()
                    .filter_map(|segment| score_candidate(&slot, segment))
                    .collect();
                candidates.sort_by(|left, right| {
                    right
                        .score
                        .cmp(&left.score)
                        .then_with(|| left.segment_id.cmp(&right.segment_id))
                });
                SlotPlan { slot, candidates }
            })
            .collect(),
    }
}

fn score_candidate(slot: &GuideSlot, segment: &Segment) -> Option<Candidate> {
    let mut score = 0;
    let mut reasons = Vec::new();
    let searchable = format!(
        "{} {} {} {}",
        segment.transcript,
        segment.keywords.join(" "),
        segment.tags.join(" "),
        segment.roles.join(" ")
    )
    .to_lowercase();
    if segment
        .roles
        .iter()
        .any(|role| role.eq_ignore_ascii_case(&slot.role))
    {
        score += 4;
        reasons.push(format!("役割「{}」が一致", slot.role));
    }
    for term in slot
        .query
        .split(|character: char| {
            character.is_whitespace() || matches!(character, ',' | '、' | '，')
        })
        .filter(|term| !term.is_empty())
    {
        if searchable.contains(&term.to_lowercase()) {
            score += 2;
            reasons.push(format!("「{}」が文字起こし・語句・タグ・役割に一致", term));
        }
    }
    (score > 0).then(|| Candidate {
        segment_id: segment.id.clone(),
        score,
        reasons,
        source_path: segment.source_path.clone(),
        start_ms: segment.start_ms,
        end_ms: segment.end_ms,
        transcript: segment.transcript.clone(),
        tags: segment.tags.clone(),
        roles: segment.roles.clone(),
    })
}

pub fn manifest_from_plan(
    plan: &Plan,
    selections: &BTreeMap<String, String>,
) -> Result<Manifest, DomainError> {
    let entries = plan
        .slots
        .iter()
        .map(|slot_plan| {
            let selection = selections
                .get(&slot_plan.slot.id)
                .ok_or_else(|| DomainError::MissingSelection(slot_plan.slot.id.clone()))?;
            let candidate = slot_plan
                .candidates
                .iter()
                .find(|candidate| candidate.segment_id == *selection)
                .ok_or_else(|| DomainError::CandidateNotFound {
                    slot: slot_plan.slot.id.clone(),
                    candidate: selection.clone(),
                })?;
            Ok(ManifestEntry {
                order: slot_plan.slot.order,
                slot_id: slot_plan.slot.id.clone(),
                role: slot_plan.slot.role.clone(),
                query: slot_plan.slot.query.clone(),
                segment_id: candidate.segment_id.clone(),
                source_path: candidate.source_path.clone(),
                start_ms: candidate.start_ms,
                end_ms: candidate.end_ms,
                selection_reason: candidate.reasons.clone(),
            })
        })
        .collect::<Result<Vec<_>, DomainError>>()?;
    Ok(Manifest {
        schema_version: SCHEMA_VERSION,
        guide_text: plan.guide_text.clone(),
        material_root: plan.material_root.clone(),
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::valueobject::Segment;
    fn catalog() -> Catalog {
        Catalog {
            schema_version: 1,
            material_root: "C:/media/cutup-samples".into(),
            segments: vec![
                Segment {
                    id: "tekkaman-intro".into(),
                    source_path: "anime/01.mp4".into(),
                    start_ms: 1200,
                    end_ms: 4500,
                    transcript: "テッカマン、参上！".into(),
                    keywords: vec!["テッカマン".into()],
                    tags: vec!["登場".into()],
                    roles: vec!["導入".into()],
                    note: String::new(),
                },
                Segment {
                    id: "surprised-reaction".into(),
                    source_path: "anime/02.mp4".into(),
                    start_ms: 8400,
                    end_ms: 10400,
                    transcript: "えっ!?".into(),
                    keywords: vec!["驚き".into()],
                    tags: vec!["反応".into()],
                    roles: vec!["つなぎ".into()],
                    note: String::new(),
                },
            ],
        }
    }
    #[test]
    fn natural_japanese_guide_becomes_ordered_slots() {
        let slots = parse_guide("テッカマンを導入に置き、驚きの反応でつなぐ");
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0].role, "導入");
        assert_eq!(slots[1].query, "驚き 反応");
    }
    #[test]
    fn selected_candidates_make_traceable_manifest() {
        let plan = create_plan(&catalog(), "導入: テッカマン\nつなぎ: 驚き 反応");
        let selections = BTreeMap::from([
            ("slot-1".to_owned(), "tekkaman-intro".to_owned()),
            ("slot-2".to_owned(), "surprised-reaction".to_owned()),
        ]);
        let manifest = manifest_from_plan(&plan, &selections).unwrap();
        assert_eq!(manifest.entries[0].source_path, "anime/01.mp4");
        assert_eq!(manifest.entries[1].start_ms, 8400);
    }
}
