use cutup_studio::{
    domain::{
        service::{create_plan, manifest_from_plan},
        valueobject::catalog::*,
    },
    infra::sqlite_catalog::{CatalogError, SqliteCatalog},
};
use std::{collections::BTreeMap, fs};
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
    store: SqliteCatalog,
    material: i64,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("素材");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("clip.wav"), b"fixture audio bytes").unwrap();
        let mut store = SqliteCatalog::create(&temp.path().join("catalog.sqlite"), &root).unwrap();
        assert_eq!(store.rescan().unwrap().added, 1);
        let material = store.materials().unwrap()[0].id;
        Self {
            temp,
            store,
            material,
        }
    }
    fn save(&mut self) -> i64 {
        self.store
            .save_manual(
                None,
                self.material,
                TimeRange::new(100, 800).unwrap(),
                &ManualMetadata {
                    transcript: Some("テッカマン登場".into()),
                    tags: Some(vec!["反応".into()]),
                    roles: Some(vec!["導入".into()]),
                    ..Default::default()
                },
            )
            .unwrap()
    }
    fn all(&self) -> Vec<StoredSegment> {
        self.store
            .search(&SearchFilter {
                include_unavailable: true,
                ..Default::default()
            })
            .unwrap()
    }
}

fn result(start: i64, end: i64, transcript: &str) -> AnalysisResult {
    AnalysisResult {
        range: TimeRange::new(start, end).unwrap(),
        metadata: Metadata {
            transcript: transcript.into(),
            speakers: vec!["話者A".into()],
            visual_tags: vec!["登場".into()],
            audio_tags: vec!["歓声".into()],
            roles: vec!["導入".into()],
            ..Default::default()
        },
        confidence: 0.8,
        evidence: "対象区間の発話と映像フレーム".into(),
    }
}

#[test]
fn folder_to_persistent_segments_to_manifest_without_copying_media() {
    let mut f = Fixture::new();
    let first = f.save();
    let second = f
        .store
        .save_manual(
            None,
            f.material,
            TimeRange::new(900, 1200).unwrap(),
            &ManualMetadata {
                transcript: Some("驚きの反応".into()),
                roles: Some(vec!["つなぎ".into()]),
                ..Default::default()
            },
        )
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(f.save(), first);
    assert_eq!(f.store.rescan().unwrap().added, 0);
    drop(f.store);
    let store = SqliteCatalog::open(&f.temp.path().join("catalog.sqlite")).unwrap();
    let catalog = store.candidate_catalog().unwrap();
    assert_eq!(catalog.segments.len(), 2);
    let plan = create_plan(&catalog, "導入: テッカマン\nつなぎ: 驚き");
    let selections = BTreeMap::from([
        ("slot-1".into(), format!("segment-{first}")),
        ("slot-2".into(), format!("segment-{second}")),
    ]);
    let manifest = manifest_from_plan(&plan, &selections).unwrap();
    assert_eq!(manifest.entries[0].source_path, "clip.wav");
    assert_eq!(manifest.entries[1].start_ms, 900);
    assert_eq!(fs::read_dir(f.temp.path()).unwrap().count(), 2);
    assert_eq!(fs::read_dir(store.root()).unwrap().count(), 1);
}

#[test]
fn rescan_detects_same_size_content_changes_and_retains_unavailable_segments() {
    let mut f = Fixture::new();
    let id = f.save();
    let path = f.store.root().join("clip.wav");
    let previous = fs::metadata(&path).unwrap();
    fs::write(&path, b"changed audio bytes").unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(previous.modified().unwrap())
        .unwrap();
    assert_eq!(f.store.rescan().unwrap().changed, 1);
    assert_eq!(f.all()[0].availability, Availability::Stale);
    assert!(f.store.candidate_catalog().unwrap().segments.is_empty());
    assert_eq!(f.save(), id);
    fs::rename(&path, f.store.root().join("moved.wav")).unwrap();
    let report = f.store.rescan().unwrap();
    assert_eq!((report.added, report.missing), (1, 1));
    assert_eq!(f.all()[0].availability, Availability::Missing);
    assert!(f.store.candidate_catalog().unwrap().segments.is_empty());
    assert!(matches!(
        f.store.save_manual(
            Some(id),
            f.material,
            TimeRange::new(100, 800).unwrap(),
            &ManualMetadata::default()
        ),
        Err(CatalogError::Unavailable)
    ));
    fs::rename(f.store.root().join("moved.wav"), &path).unwrap();
    assert_eq!(f.store.rescan().unwrap().restored, 1);
    assert_eq!(f.all()[0].id, id);
    assert_eq!(f.all()[0].availability, Availability::Available);
}

#[test]
fn manual_overrides_win_in_display_fts_and_filters_after_ai_rerun() {
    let mut f = Fixture::new();
    let job = f
        .store
        .start_analysis(f.material, "local-model", "v1")
        .unwrap();
    f.store
        .complete_analysis(job, &[result(100, 800, "自動推定の文字起こし")])
        .unwrap();
    let id = f.all()[0].id;
    f.store
        .save_manual(
            Some(id),
            f.material,
            TimeRange::new(100, 800).unwrap(),
            &ManualMetadata {
                transcript: Some("修正テキスト".into()),
                visual_tags: Some(vec![]),
                tags: Some(vec!["手動タグ".into()]),
                ..Default::default()
            },
        )
        .unwrap();
    let job = f
        .store
        .start_analysis(f.material, "local-model", "v2")
        .unwrap();
    f.store
        .complete_analysis(job, &[result(100, 800, "再解析の文字起こし")])
        .unwrap();
    let rows = f
        .store
        .search(&SearchFilter {
            text: "修正テキスト",
            tag: Some("手動タグ"),
            role: Some("導入"),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    assert!(rows[0].metadata.visual_tags.is_empty());
    assert_eq!(rows[0].metadata.audio_tags, vec!["歓声"]);
    assert!(
        f.store
            .search(&SearchFilter {
                text: "再解析",
                ..Default::default()
            })
            .unwrap()
            .is_empty()
    );
    assert!(
        f.store
            .search(&SearchFilter {
                tag: Some("登場"),
                ..Default::default()
            })
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.store.analysis_history(id).unwrap().len(), 2);
    assert_eq!(f.store.analysis_history(id).unwrap()[0].version, "v2");
    assert!(f.store.analysis_targets().unwrap().is_empty());
    assert_eq!(f.store.rescan().unwrap().changed, 0);
    assert!(f.store.analysis_targets().unwrap().is_empty());
}

#[test]
fn japanese_short_queries_and_literal_fts_punctuation_are_searchable() {
    let mut f = Fixture::new();
    f.save();
    assert_eq!(
        f.store
            .search(&SearchFilter {
                text: "反応",
                ..Default::default()
            })
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        f.store
            .search(&SearchFilter {
                text: "テッカマン",
                ..Default::default()
            })
            .unwrap()
            .len(),
        1
    );
    assert!(
        f.store
            .search(&SearchFilter {
                text: "\" OR *",
                ..Default::default()
            })
            .unwrap()
            .is_empty()
    );
    assert!(
        f.store
            .search(&SearchFilter {
                text: "%",
                ..Default::default()
            })
            .unwrap()
            .is_empty()
    );
    f.store
        .save_manual(
            None,
            f.material,
            TimeRange::new(100, 800).unwrap(),
            &ManualMetadata {
                transcript: Some("別の言葉".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        f.store
            .search(&SearchFilter {
                text: "テッカマン",
                ..Default::default()
            })
            .unwrap()
            .is_empty()
    );
}

#[test]
fn changed_inputs_supersede_running_jobs_and_require_reanalysis() {
    let mut f = Fixture::new();
    let job = f.store.start_analysis(f.material, "model", "v1").unwrap();
    fs::write(f.store.root().join("clip.wav"), b"different content").unwrap();
    f.store.rescan().unwrap();
    assert!(matches!(
        f.store
            .complete_analysis(job, &[result(0, 100, "古い結果")]),
        Err(CatalogError::InvalidJob)
    ));
    assert_eq!(f.store.analysis_targets().unwrap(), vec![f.material]);
    assert!(f.all().is_empty());
    let next = f.store.start_analysis(f.material, "model", "v1").unwrap();
    f.store.fail_analysis(next, "model unavailable").unwrap();
    assert_eq!(f.store.analysis_targets().unwrap(), vec![f.material]);
    let next = f.store.start_analysis(f.material, "model", "v1").unwrap();
    f.store
        .complete_analysis(next, &[result(0, 100, "最新の結果")])
        .unwrap();
    assert!(f.store.analysis_targets().unwrap().is_empty());
}

#[test]
fn reanalysis_retires_orphan_ai_ranges_but_preserves_manual_ranges() {
    let mut f = Fixture::new();
    let manual_id = f.save();
    let job = f.store.start_analysis(f.material, "model", "v1").unwrap();
    f.store
        .complete_analysis(job, &[result(0, 100, "旧区間")])
        .unwrap();
    let job = f.store.start_analysis(f.material, "model", "v2").unwrap();
    f.store
        .complete_analysis(job, &[result(900, 1000, "新区間")])
        .unwrap();
    assert_eq!(f.all().len(), 3);
    assert_eq!(
        f.all()
            .iter()
            .filter(|s| s.availability == Availability::Stale)
            .count(),
        1
    );
    let current = f.store.candidate_catalog().unwrap();
    assert_eq!(current.segments.len(), 2);
    assert!(
        current
            .segments
            .iter()
            .any(|s| s.id == format!("segment-{manual_id}"))
    );
}

#[test]
fn invalid_or_duplicate_analysis_results_do_not_partially_commit() {
    let mut f = Fixture::new();
    let job = f.store.start_analysis(f.material, "model", "v1").unwrap();
    let valid = result(0, 100, "valid");
    let mut invalid = result(100, 200, "invalid");
    invalid.confidence = f64::NAN;
    assert!(matches!(
        f.store.complete_analysis(job, &[valid.clone(), invalid]),
        Err(CatalogError::Metadata(MetadataError::InvalidProvenance))
    ));
    assert!(f.all().is_empty());
    assert!(matches!(
        f.store
            .complete_analysis(job, &[valid.clone(), valid.clone()]),
        Err(CatalogError::Sql(_))
    ));
    assert!(f.all().is_empty());
    f.store.complete_analysis(job, &[valid]).unwrap();
    assert_eq!(f.all().len(), 1);
    assert!(matches!(
        TimeRange::new(5, 5),
        Err(MetadataError::InvalidRange)
    ));
    assert!(matches!(
        TimeRange::new(-1, 5),
        Err(MetadataError::InvalidRange)
    ));
}

#[test]
fn failed_scan_keeps_previous_records_and_unsupported_databases_are_rejected() {
    let mut f = Fixture::new();
    f.save();
    let root = f.store.root().to_path_buf();
    fs::rename(&root, f.temp.path().join("moved-root")).unwrap();
    fs::write(&root, b"not a directory").unwrap();
    assert!(matches!(f.store.rescan(), Err(CatalogError::Io(_))));
    assert_eq!(f.all()[0].availability, Availability::Available);
    let db = f.temp.path().join("catalog.sqlite");
    assert!(matches!(
        SqliteCatalog::create(&db, &f.temp.path().join("moved-root")),
        Err(CatalogError::Io(_))
    ));
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection.pragma_update(None, "user_version", 999).unwrap();
    assert!(matches!(
        SqliteCatalog::open(&db),
        Err(CatalogError::UnsupportedSchema)
    ));
    assert!(!f.temp.path().join("absent.sqlite").exists());
    assert!(matches!(
        SqliteCatalog::open(&f.temp.path().join("absent.sqlite")),
        Err(CatalogError::Sql(_))
    ));
    assert!(!f.temp.path().join("absent.sqlite").exists());
}

#[test]
fn recursive_scan_ignores_nonmedia_and_keeps_distinct_paths_with_identical_content() {
    let mut f = Fixture::new();
    fs::create_dir(f.store.root().join("nested")).unwrap();
    fs::write(
        f.store.root().join("nested/CLIP.MP4"),
        b"fixture audio bytes",
    )
    .unwrap();
    fs::write(f.store.root().join("ignore.txt"), b"not media").unwrap();
    assert_eq!(f.store.rescan().unwrap().added, 1);
    let materials = f.store.materials().unwrap();
    assert_eq!(materials.len(), 2);
    assert_eq!(materials[0].fingerprint, materials[1].fingerprint);
    assert_ne!(materials[0].id, materials[1].id);
    assert_eq!(materials[1].source_path, "nested/CLIP.MP4");
}

#[test]
fn missing_root_is_reported_as_unavailable_and_restoring_it_preserves_ids() {
    let mut f = Fixture::new();
    let id = f.save();
    let root = f.store.root().to_path_buf();
    let moved = f.temp.path().join("temporarily-moved");
    fs::rename(&root, &moved).unwrap();
    assert_eq!(f.store.rescan().unwrap().missing, 1);
    assert_eq!(f.all()[0].availability, Availability::Missing);
    assert!(f.store.candidate_catalog().unwrap().segments.is_empty());
    fs::rename(&moved, &root).unwrap();
    assert_eq!(f.store.rescan().unwrap().restored, 1);
    assert_eq!(f.all()[0].id, id);
}

#[test]
fn correcting_time_range_preserves_id_and_rejects_collisions() {
    let mut f = Fixture::new();
    let id = f.save();
    f.store
        .save_manual(
            Some(id),
            f.material,
            TimeRange::new(200, 500).unwrap(),
            &ManualMetadata::default(),
        )
        .unwrap();
    assert_eq!(f.all()[0].id, id);
    assert_eq!(f.all()[0].range, TimeRange::new(200, 500).unwrap());
    let other = f
        .store
        .save_manual(
            None,
            f.material,
            TimeRange::new(600, 700).unwrap(),
            &ManualMetadata::default(),
        )
        .unwrap();
    assert!(matches!(
        f.store.save_manual(
            Some(id),
            f.material,
            TimeRange::new(600, 700).unwrap(),
            &ManualMetadata::default()
        ),
        Err(CatalogError::Sql(_))
    ));
    assert_eq!(f.all().len(), 2);
    assert_eq!(f.all()[0].range, TimeRange::new(200, 500).unwrap());
    assert_eq!(f.all()[1].id, other);
    assert!(serde_json::from_str::<TimeRange>(r#"{"start_ms":9,"end_ms":1}"#).is_err());
}

#[test]
fn manually_confirmed_ai_range_survives_reanalysis_even_without_text_overrides() {
    let mut f = Fixture::new();
    let job = f.store.start_analysis(f.material, "model", "v1").unwrap();
    f.store
        .complete_analysis(job, &[result(100, 800, "確認した区間")])
        .unwrap();
    let id = f.all()[0].id;
    f.store
        .save_manual(
            Some(id),
            f.material,
            TimeRange::new(100, 800).unwrap(),
            &ManualMetadata::default(),
        )
        .unwrap();
    let job = f.store.start_analysis(f.material, "model", "v2").unwrap();
    f.store.complete_analysis(job, &[]).unwrap();
    assert_eq!(f.store.candidate_catalog().unwrap().segments.len(), 1);
    assert_eq!(f.all()[0].id, id);
}

#[test]
fn directory_links_cannot_escape_the_root_or_create_scan_cycles() {
    let mut f = Fixture::new();
    let link = f.store.root().join("parent-link");
    #[cfg(windows)]
    {
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(f.temp.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(f.temp.path(), &link).unwrap();
    assert_eq!(f.store.rescan().unwrap().added, 0);
    assert_eq!(f.store.materials().unwrap().len(), 1);
    #[cfg(windows)]
    fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    fs::remove_file(&link).unwrap();
}
