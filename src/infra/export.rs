use crate::domain::valueobject::{Catalog, CompositionRecipe};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn inputs(catalog: &Catalog, recipe: &CompositionRecipe) -> Result<Vec<PathBuf>, String> {
    if recipe.slots.is_empty() {
        return Err("保存済みの構成が空です。".into());
    }
    let root = Path::new(&catalog.material_root)
        .join("materials")
        .canonicalize()
        .map_err(|e| format!("素材フォルダを開けません: {e}"))?;
    let mut slots: Vec<_> = recipe.slots.iter().collect();
    slots.sort_by_key(|slot| slot.slot.order);
    slots
        .into_iter()
        .map(|slot| {
            let selection = slot.selection.as_ref().ok_or_else(|| {
                format!(
                    "{}番の素材が未選択です。02で選んで保存してください。",
                    slot.slot.order
                )
            })?;
            let material = catalog
                .materials
                .iter()
                .find(|m| m.id == selection.material_id)
                .ok_or_else(|| format!("{}番の素材がカタログにありません。", slot.slot.order))?;
            let path = Path::new(&catalog.material_root)
                .join(&material.source_path)
                .canonicalize()
                .map_err(|_| format!("素材が見つかりません: {}", material.source_path))?;
            if !path.starts_with(&root) || !path.is_file() {
                return Err(format!("素材を読み取れません: {}", material.source_path));
            }
            Ok(path)
        })
        .collect()
}

pub fn render(ffmpeg: &Path, inputs: &[PathBuf], output: &Path) -> Result<(), String> {
    if inputs.is_empty() {
        return Err("書き出す素材がありません。".into());
    }
    let parent = output.parent().ok_or("保存先が不正です。")?;
    let temp = tempfile::Builder::new()
        .suffix(".wav")
        .tempfile_in(parent)
        .map_err(|e| format!("一時ファイルを作成できません: {e}"))?;
    let mut command = Command::new(ffmpeg);
    command.args(["-nostdin", "-v", "error", "-xerror", "-y"]);
    for input in inputs {
        command.arg("-i").arg(input);
    }
    let mut filter = String::new();
    for index in 0..inputs.len() {
        filter.push_str(&format!(
            "[{index}:a:0]aresample=48000,aformat=sample_fmts=s16:channel_layouts=stereo[a{index}];"
        ));
    }
    for index in 0..inputs.len() {
        filter.push_str(&format!("[a{index}]"));
    }
    filter.push_str(&format!("concat=n={}:v=0:a=1[out]", inputs.len()));
    let result = command
        .args([
            "-filter_complex",
            &filter,
            "-map",
            "[out]",
            "-c:a",
            "pcm_s16le",
            "-f",
            "wav",
        ])
        .arg(temp.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .map_err(|e| {
            format!("FFmpegを起動できません: {e}。FFmpegを導入し実行ファイルを指定してください。")
        })?;
    if !result.status.success() {
        return Err(format!(
            "FFmpegで書き出せません: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    temp.persist_noclobber(output).map_err(|e| {
        format!(
            "保存できません（既存ファイルは上書きしません）: {}",
            e.error
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_recipe_resolves_order_and_rejects_missing_selection() {
        use crate::domain::valueobject::{
            CatalogMaterial, CompositionRecipeSlot, GuideSlot, RecipeSelection,
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("materials")).unwrap();
        for name in ["a.wav", "b.wav"] {
            std::fs::write(dir.path().join("materials").join(name), b"test").unwrap();
        }
        let material = |id: &str, name: &str| CatalogMaterial {
            id: id.into(),
            source_path: format!("materials/{name}"),
            keywords: vec![],
            tags: vec![],
            roles: vec![],
            note: String::new(),
        };
        let catalog = Catalog {
            schema_version: 1,
            material_root: dir.path().to_string_lossy().into_owned(),
            materials: vec![material("1", "a.wav"), material("2", "b.wav")],
        };
        let slot = |order: usize, id: &str| CompositionRecipeSlot {
            slot: GuideSlot {
                id: order.to_string(),
                order,
                role: String::new(),
                query: String::new(),
            },
            selection: Some(RecipeSelection {
                material_id: id.into(),
                score: 0,
                reasons: vec![],
            }),
        };
        let mut recipe = CompositionRecipe {
            slots: vec![slot(2, "2"), slot(1, "1")],
        };
        let paths = inputs(&catalog, &recipe).unwrap();
        assert!(paths[0].ends_with("a.wav"));
        assert!(paths[1].ends_with("b.wav"));
        recipe.slots[0].selection = None;
        assert!(inputs(&catalog, &recipe).unwrap_err().contains("未選択"));
        std::fs::remove_file(dir.path().join("materials/a.wav")).unwrap();
        recipe.slots.remove(0);
        assert!(
            inputs(&catalog, &recipe)
                .unwrap_err()
                .contains("見つかりません")
        );
    }

    #[test]
    fn missing_ffmpeg_does_not_create_output() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("output.wav");
        let result = render(
            &dir.path().join("missing"),
            &[PathBuf::from("input.wav")],
            &output,
        );
        assert!(result.unwrap_err().contains("FFmpegを起動できません"));
        assert!(!output.exists());
    }

    #[test]
    #[ignore = "requires FFmpeg on PATH"]
    fn ffmpeg_concatenates_demo_audio_and_rejects_invalid_input() {
        let dir = tempfile::tempdir().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/materials");
        let output = dir.path().join("output.wav");
        render(
            Path::new("ffmpeg"),
            &[root.join("01-tekkaman.wav"), root.join("02-ringo.wav")],
            &output,
        )
        .unwrap();
        assert!(output.metadata().unwrap().len() > 44);
        let original = std::fs::read(&output).unwrap();
        assert!(
            render(
                Path::new("ffmpeg"),
                &[root.join("01-tekkaman.wav")],
                &output
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&output).unwrap(), original);
        let failed = dir.path().join("failed.wav");
        assert!(
            render(
                Path::new("ffmpeg"),
                &[root.join("01-tekkaman.wav"), dir.path().join("missing.wav")],
                &failed
            )
            .is_err()
        );
        assert!(!failed.exists());
    }
}
