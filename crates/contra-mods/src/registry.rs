use crate::manifest::ModManifest;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct InstalledMod {
    pub manifest: ModManifest,
    pub dir: PathBuf,
    pub enabled: bool,
}

/// Discovers mods under a `/mods/` directory and tracks which are enabled,
/// in load order. Load order matters for asset overrides (later mods win)
/// and for Lua mods that depend on another mod's `id` via `requires`.
#[derive(Debug, Default)]
pub struct ModRegistry {
    mods: Vec<InstalledMod>,
}

impl ModRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Scans `mods_dir` for subdirectories containing a `mod.toml`.
    /// Unreadable/invalid mods are skipped, not fatal to the whole scan.
    pub fn scan(mods_dir: impl AsRef<Path>) -> Self {
        let mut registry = Self::new();
        let Ok(entries) = std::fs::read_dir(mods_dir) else {
            return registry;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            if let Ok(manifest) = ModManifest::load(&dir) {
                registry.mods.push(InstalledMod { manifest, dir, enabled: false });
            }
        }
        registry
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        if let Some(m) = self.mods.iter_mut().find(|m| m.manifest.id == id) {
            m.enabled = enabled;
        }
    }

    pub fn enabled_in_load_order(&self) -> impl Iterator<Item = &InstalledMod> {
        self.mods.iter().filter(|m| m.enabled)
    }

    pub fn all(&self) -> &[InstalledMod] {
        &self.mods
    }

    /// Checks that every enabled mod's `requires` list is also enabled.
    /// Returns the ids of mods with unmet dependencies.
    pub fn unmet_dependencies(&self) -> Vec<String> {
        let enabled_ids: std::collections::HashSet<&str> =
            self.enabled_in_load_order().map(|m| m.manifest.id.as_str()).collect();
        self.enabled_in_load_order()
            .filter(|m| !m.manifest.requires.iter().all(|dep| enabled_ids.contains(dep.as_str())))
            .map(|m| m.manifest.id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scans_a_directory_of_mods() {
        let tmp = std::env::temp_dir().join(format!("contra_mods_test_{}", std::process::id()));
        let mod_a = tmp.join("mod-a");
        fs::create_dir_all(&mod_a).unwrap();
        fs::write(
            mod_a.join("mod.toml"),
            r#"id = "mod-a"
name = "Mod A"
version = "1.0.0"
author = "test""#,
        )
        .unwrap();

        let mut registry = ModRegistry::scan(&tmp);
        assert_eq!(registry.all().len(), 1);
        registry.set_enabled("mod-a", true);
        assert_eq!(registry.enabled_in_load_order().count(), 1);

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn detects_unmet_dependencies() {
        let mut registry = ModRegistry::new();
        registry.mods.push(InstalledMod {
            manifest: ModManifest {
                id: "needs-base".into(),
                name: "n".into(),
                version: "1.0.0".into(),
                author: "a".into(),
                description: String::new(),
                entry_script: None,
                sprite_overrides: vec![],
                music_overrides: vec![],
                level_files: vec![],
                requires: vec!["base-pack".into()],
            },
            dir: PathBuf::new(),
            enabled: true,
        });
        assert_eq!(registry.unmet_dependencies(), vec!["needs-base".to_string()]);
    }
}
