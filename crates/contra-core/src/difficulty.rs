//! Difficulty presets and the fully-custom slider set, plus a shareable
//! text code (e.g. `CONTRA-4XHP-NOCONTINUE-200ENEMIES`) so players can trade
//! rulesets without sending a config file.

use serde::{Deserialize, Serialize};

use crate::checkpoint::CheckpointMode;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
    VeryHard,
    Nightmare,
    Custom,
}

impl Default for Difficulty {
    fn default() -> Self {
        Difficulty::Normal
    }
}

/// Every slider/toggle exposed by "Custom Difficulty". `Difficulty::Normal`
/// corresponds to all multipliers at `1.0` and matches NES behavior;
/// presets (Easy/Hard/...) are just different starting values for the same
/// struct, so the UI can show a preset and then let the player nudge it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomDifficulty {
    pub starting_lives: u8,
    pub continues: Continues,
    pub projectile_speed_mult: f32,
    pub enemy_speed_mult: f32,
    pub enemy_density_mult: f32,
    pub spawn_rate_mult: f32,
    pub damage_mult: f32,
    pub checkpoint_mode: CheckpointMode,
    pub weapon_drop_mult: f32,
    /// Frames of invincibility after taking a hit. NES default: see
    /// `NEW_LIFE_INVINCIBILITY_TIMER` / `INVINCIBILITY_TIMER` in ram.asm.
    pub post_hit_invincibility_frames: u16,
    pub boss_hp_mult: f32,
    pub friendly_fire: bool,
    pub permadeath: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Continues {
    Unlimited,
    Limited(u8),
    None,
}

impl CustomDifficulty {
    pub fn preset(difficulty: Difficulty) -> Self {
        match difficulty {
            Difficulty::Easy => Self {
                starting_lives: 5,
                continues: Continues::Unlimited,
                projectile_speed_mult: 0.85,
                enemy_speed_mult: 0.85,
                enemy_density_mult: 0.85,
                spawn_rate_mult: 0.85,
                damage_mult: 0.75,
                checkpoint_mode: CheckpointMode::Casual,
                weapon_drop_mult: 1.25,
                post_hit_invincibility_frames: 120,
                boss_hp_mult: 0.85,
                friendly_fire: false,
                permadeath: false,
            },
            Difficulty::Normal => Self::original(),
            Difficulty::Hard => Self {
                starting_lives: 3,
                continues: Continues::Limited(3),
                projectile_speed_mult: 1.2,
                enemy_speed_mult: 1.15,
                enemy_density_mult: 1.25,
                spawn_rate_mult: 1.2,
                damage_mult: 1.0,
                checkpoint_mode: CheckpointMode::Original,
                weapon_drop_mult: 0.85,
                post_hit_invincibility_frames: 80,
                boss_hp_mult: 1.25,
                friendly_fire: false,
                permadeath: false,
            },
            Difficulty::VeryHard => Self {
                starting_lives: 2,
                continues: Continues::Limited(1),
                projectile_speed_mult: 1.4,
                enemy_speed_mult: 1.3,
                enemy_density_mult: 1.5,
                spawn_rate_mult: 1.4,
                damage_mult: 1.25,
                checkpoint_mode: CheckpointMode::Original,
                weapon_drop_mult: 0.7,
                post_hit_invincibility_frames: 60,
                boss_hp_mult: 1.5,
                friendly_fire: false,
                permadeath: false,
            },
            Difficulty::Nightmare => Self {
                starting_lives: 1,
                continues: Continues::None,
                projectile_speed_mult: 1.75,
                enemy_speed_mult: 1.5,
                enemy_density_mult: 2.0,
                spawn_rate_mult: 1.75,
                damage_mult: 2.0,
                checkpoint_mode: CheckpointMode::Original,
                weapon_drop_mult: 0.5,
                post_hit_invincibility_frames: 30,
                boss_hp_mult: 2.0,
                friendly_fire: true,
                permadeath: true,
            },
            Difficulty::Custom => Self::original(),
        }
    }

    /// All multipliers at 1.0 - the NES original, unmodified.
    pub fn original() -> Self {
        Self {
            starting_lives: 3,
            continues: Continues::Limited(2),
            projectile_speed_mult: 1.0,
            enemy_speed_mult: 1.0,
            enemy_density_mult: 1.0,
            spawn_rate_mult: 1.0,
            damage_mult: 1.0,
            checkpoint_mode: CheckpointMode::Original,
            weapon_drop_mult: 1.0,
            post_hit_invincibility_frames: 100,
            boss_hp_mult: 1.0,
            friendly_fire: false,
            permadeath: false,
        }
    }

    /// Encodes to a human-shareable string, e.g.
    /// `CONTRA-L1-C0-DMGx200-BOSSx150-ENEMYx120-CHKoriginal-PERMADEATH`.
    /// Only fields that differ from [`Self::original`] are included, so
    /// simple rulesets stay short.
    pub fn to_code(&self) -> String {
        let base = Self::original();
        let mut parts = vec!["CONTRA".to_string()];

        if self.starting_lives != base.starting_lives {
            parts.push(format!("L{}", self.starting_lives));
        }
        match self.continues {
            Continues::Unlimited => parts.push("CUNLIMITED".to_string()),
            Continues::None => parts.push("NOCONTINUE".to_string()),
            Continues::Limited(n) if Continues::Limited(n) != base.continues => {
                parts.push(format!("C{n}"));
            }
            _ => {}
        }
        push_pct(&mut parts, "PROJ", self.projectile_speed_mult, base.projectile_speed_mult);
        push_pct(&mut parts, "ESPD", self.enemy_speed_mult, base.enemy_speed_mult);
        push_pct(&mut parts, "EDEN", self.enemy_density_mult, base.enemy_density_mult);
        push_pct(&mut parts, "SPWN", self.spawn_rate_mult, base.spawn_rate_mult);
        push_pct(&mut parts, "DMG", self.damage_mult, base.damage_mult);
        push_pct(&mut parts, "DROP", self.weapon_drop_mult, base.weapon_drop_mult);
        push_pct(&mut parts, "BOSSHP", self.boss_hp_mult, base.boss_hp_mult);

        if self.checkpoint_mode != base.checkpoint_mode {
            parts.push(format!("{:?}", self.checkpoint_mode).to_uppercase());
        }
        if self.post_hit_invincibility_frames != base.post_hit_invincibility_frames {
            parts.push(format!("INV{}", self.post_hit_invincibility_frames));
        }
        if self.friendly_fire {
            parts.push("FRIENDLYFIRE".to_string());
        }
        if self.permadeath {
            parts.push("PERMADEATH".to_string());
        }

        parts.join("-")
    }

    /// Parses a code produced by [`Self::to_code`]. Unknown/malformed
    /// tokens are ignored rather than erroring, so codes stay forward
    /// compatible as new sliders are added.
    pub fn from_code(code: &str) -> Self {
        let mut d = Self::original();
        for token in code.split('-').skip(1) {
            // skip the leading "CONTRA" prefix, if present
            if token.eq_ignore_ascii_case("CONTRA") {
                continue;
            }
            if let Some(n) = parse_prefixed_u8(token, "L") {
                d.starting_lives = n;
            } else if token.eq_ignore_ascii_case("CUNLIMITED") {
                d.continues = Continues::Unlimited;
            } else if token.eq_ignore_ascii_case("NOCONTINUE") {
                d.continues = Continues::None;
            } else if let Some(n) = parse_prefixed_u8(token, "C") {
                d.continues = Continues::Limited(n);
            } else if let Some(v) = parse_prefixed_pct(token, "PROJ") {
                d.projectile_speed_mult = v;
            } else if let Some(v) = parse_prefixed_pct(token, "ESPD") {
                d.enemy_speed_mult = v;
            } else if let Some(v) = parse_prefixed_pct(token, "EDEN") {
                d.enemy_density_mult = v;
            } else if let Some(v) = parse_prefixed_pct(token, "SPWN") {
                d.spawn_rate_mult = v;
            } else if let Some(v) = parse_prefixed_pct(token, "DMG") {
                d.damage_mult = v;
            } else if let Some(v) = parse_prefixed_pct(token, "DROP") {
                d.weapon_drop_mult = v;
            } else if let Some(v) = parse_prefixed_pct(token, "BOSSHP") {
                d.boss_hp_mult = v;
            } else if let Some(n) = parse_prefixed_u16(token, "INV") {
                d.post_hit_invincibility_frames = n;
            } else if token.eq_ignore_ascii_case("FRIENDLYFIRE") {
                d.friendly_fire = true;
            } else if token.eq_ignore_ascii_case("PERMADEATH") {
                d.permadeath = true;
            } else if token.eq_ignore_ascii_case("ORIGINAL") {
                d.checkpoint_mode = CheckpointMode::Original;
            } else if token.eq_ignore_ascii_case("CASUAL") {
                d.checkpoint_mode = CheckpointMode::Casual;
            } else if token.eq_ignore_ascii_case("MODERN") {
                d.checkpoint_mode = CheckpointMode::Modern;
            } else if token.eq_ignore_ascii_case("PRACTICE") {
                d.checkpoint_mode = CheckpointMode::Practice;
            }
        }
        d
    }
}

fn push_pct(parts: &mut Vec<String>, tag: &str, value: f32, base: f32) {
    if (value - base).abs() > f32::EPSILON {
        parts.push(format!("{tag}{}", (value * 100.0).round() as i32));
    }
}

fn parse_prefixed_u8(token: &str, prefix: &str) -> Option<u8> {
    token.strip_prefix(prefix)?.parse().ok()
}

fn parse_prefixed_u16(token: &str, prefix: &str) -> Option<u16> {
    token.strip_prefix(prefix)?.parse().ok()
}

fn parse_prefixed_pct(token: &str, prefix: &str) -> Option<f32> {
    let n: i32 = token.strip_prefix(prefix)?.parse().ok()?;
    Some(n as f32 / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_code() {
        let mut d = CustomDifficulty::original();
        d.boss_hp_mult = 4.0;
        d.continues = Continues::None;
        d.enemy_density_mult = 2.0;

        let code = d.to_code();
        let parsed = CustomDifficulty::from_code(&code);
        assert_eq!(d, parsed);
    }

    #[test]
    fn matches_documented_example_shape() {
        let mut d = CustomDifficulty::original();
        d.boss_hp_mult = 4.0;
        d.continues = Continues::None;
        d.enemy_density_mult = 2.0;
        let code = d.to_code();
        assert!(code.starts_with("CONTRA-"));
        assert!(code.contains("NOCONTINUE"));
        assert!(code.contains("BOSSHP400"));
        assert!(code.contains("EDEN200"));
    }
}
