use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// Relative path to a Lua entry script, if this mod has behavior
    /// beyond asset replacement. Requires the host to be built with the
    /// `lua` feature.
    #[serde(default)]
    pub entry_script: Option<PathBuf>,
    #[serde(default)]
    pub sprite_overrides: Vec<PathBuf>,
    #[serde(default)]
    pub music_overrides: Vec<PathBuf>,
    #[serde(default)]
    pub level_files: Vec<PathBuf>,
    #[serde(default)]
    pub requires: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("could not read mod.toml: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not parse mod.toml: {0}")]
    Parse(#[from] toml::de::Error),
}

impl ModManifest {
    pub fn load(mod_dir: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let path = mod_dir.as_ref().join("mod.toml");
        let text = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_minimal_manifest() {
        let toml_text = r#"
            id = "hd-sprites"
            name = "HD Sprites"
            version = "1.0.0"
            author = "someone"
        "#;
        let m: ModManifest = toml::from_str(toml_text).unwrap();
        assert_eq!(m.id, "hd-sprites");
        assert!(m.sprite_overrides.is_empty());
        assert!(m.entry_script.is_none());
    }
}
