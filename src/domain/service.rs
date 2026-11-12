use super::valueobject::{Candidate, Catalog, CatalogMaterial, GuideSlot, Plan, SlotPlan};

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
        slots: parse_guide(guide_text)
            .into_iter()
            .map(|slot| {
                let mut candidates: Vec<_> = catalog
                    .materials
                    .iter()
                    .filter_map(|material| score_candidate(&slot, material))
                    .collect();
                candidates.sort_by(|left, right| {
                    right
                        .score
                        .cmp(&left.score)
                        .then_with(|| left.material_id.cmp(&right.material_id))
                });
                SlotPlan { slot, candidates }
            })
            .collect(),
    }
}

fn score_candidate(slot: &GuideSlot, material: &CatalogMaterial) -> Option<Candidate> {
    if !material
        .roles
        .iter()
        .any(|role| role.eq_ignore_ascii_case(&slot.role))
    {
        return None;
    }
    let mut score = 0;
    let mut reasons = vec![format!("役割「{}」が一致", slot.role)];
    let file_name = material
        .source_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&material.source_path);
    let normalized_file_name = file_name.to_lowercase();
    for term in slot
        .query
        .split(|character: char| {
            character.is_whitespace() || matches!(character, ',' | '、' | '，')
        })
        .filter(|term| !term.is_empty())
    {
        let normalized_term = term.to_lowercase();
        if normalized_file_name.contains(&normalized_term) {
            score += 2;
            reasons.push(format!("ファイル名「{file_name}」に「{term}」が一致"));
        }
        for (label, values) in [
            ("検索語句", material.keywords.as_slice()),
            ("タグ", material.tags.as_slice()),
            ("役割", material.roles.as_slice()),
            ("メモ", std::slice::from_ref(&material.note)),
        ] {
            if values
                .iter()
                .any(|value| value.to_lowercase().contains(&normalized_term))
            {
                score += 2;
                reasons.push(format!("{label}に「{term}」が一致"));
            }
        }
    }
    (score > 0).then(|| Candidate {
        material_id: material.id.clone(),
        score,
        reasons,
        source_path: material.source_path.clone(),
        tags: material.tags.clone(),
        roles: material.roles.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Catalog {
        Catalog {
            schema_version: 1,
            material_root: "C:/media/cutup-samples".into(),
            materials: vec![
                CatalogMaterial {
                    id: "tekkaman".into(),
                    source_path: "anime/01-tekkaman.mp4".into(),
                    keywords: vec!["テッカマン".into()],
                    tags: vec!["登場".into()],
                    roles: vec!["導入".into()],
                    note: String::new(),
                },
                CatalogMaterial {
                    id: "surprised-reaction".into(),
                    source_path: "anime/02.mp4".into(),
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
    fn candidate_requires_matching_role_and_query() {
        let plan = create_plan(&catalog(), "導入: t");
        let candidate = &plan.slots[0].candidates[0];
        assert_eq!(candidate.material_id, "tekkaman");
        assert_eq!(
            candidate.reasons,
            [
                "役割「導入」が一致",
                "ファイル名「01-tekkaman.mp4」に「t」が一致"
            ]
        );

        let plan = create_plan(&catalog(), "つなぎ: t");
        assert!(plan.slots[0].candidates.is_empty());
    }

    #[test]
    fn multiple_slots_show_matching_fields_and_allow_the_same_material() {
        let plan = create_plan(&catalog(), "導入: テッカマン 登場\n導入: tekkaman");
        assert_eq!(plan.slots.len(), 2);
        assert_eq!(plan.slots[0].candidates.len(), 1);
        assert_eq!(plan.slots[1].candidates.len(), 1);
        assert_eq!(
            plan.slots[0].candidates[0].material_id,
            plan.slots[1].candidates[0].material_id
        );
        assert!(
            plan.slots[0].candidates[0]
                .reasons
                .contains(&"検索語句に「テッカマン」が一致".to_owned())
        );
        assert!(
            plan.slots[0].candidates[0]
                .reasons
                .contains(&"タグに「登場」が一致".to_owned())
        );
    }
}
