//! A finer index than the kind: each resource gets a category path such as `Weapons/Primary` or
//! `Enemies/Terminids`, read from the game name. Textures rarely have names, so a texture takes the
//! category of the model whose material uses it. Weapon classes follow the Helldivers 1 loadout
//! (primary, sidearm, support weapon from a stratagem).

use std::collections::HashMap;

use rayon::prelude::*;

use crate::archive::Archive;
use crate::export::{Kind, kind_of};
use crate::hash::{self, NameDb};
use crate::links::Linker;

pub const UNNAMED: &str = "Unnamed";

/// Weapons a stratagem drops (support weapons), by the part of the name after `equipment/wep_`.
const SUPPORT: &[&str] = &[
    "20mm_cannon", "chemthrower", "flamethrower", "grenadelauncher", "lasercannon", "law", "mg_01", "mg_02", "metaldetector",
    "missile_launcher", "mortar", "repairtool", "rpg", "satchelcharge",
];
const SIDEARM: &[&str] = &["pistol_01", "revolver_01", "flamepistol_01", "plasmapistol_01"];
const BACKPACK: &[&str] = &["ammo_backpack"];

fn cap(s: &str) -> String {
    let s = s.replace('_', " ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn weapon(stem: &str) -> &'static str {
    if stem.contains("pickup") {
        return "Weapons/Pickups";
    }
    if BACKPACK.iter().any(|b| stem.starts_with(b)) {
        return "Equipment/Backpacks";
    }
    if SIDEARM.iter().any(|b| stem.starts_with(b)) {
        return "Weapons/Sidearms";
    }
    if SUPPORT.iter().any(|b| stem.starts_with(b)) {
        return "Weapons/Support";
    }
    "Weapons/Primary"
}

fn equipment(stem: &str) -> &'static str {
    if stem.contains("grenade") || stem.contains("satchel") {
        "Equipment/Grenades"
    } else if stem.contains("backpack") || stem.contains("jumppack") || stem.contains("generator_pack") || stem.contains("drone") {
        "Equipment/Backpacks"
    } else if stem.contains("shield") {
        "Equipment/Shields"
    } else {
        "Equipment/Other"
    }
}

fn gameobject(stem: &str) -> &'static str {
    let s = stem.trim_start_matches("gameobject_");
    let any = |list: &[&str]| list.iter().any(|w| s.contains(w));
    if s.starts_with("supplycrate") || s.starts_with("pickup_") || s.contains("carry_crate") {
        "Stratagems/Supply"
    } else if any(&["mechsuit", "bike", "tankdestroyer", "infantryvehicle", "shuttle"]) {
        "Vehicles"
    } else if any(&["hellpod", "turret", "stationary_weapon", "tesla"]) {
        "Stratagems/Hellpods and turrets"
    } else if any(&["strike", "artillery", "strafing", "napalm", "missile", "orbital", "dive_bomb", "mine", "flare", "barricade", "barbwire", "uav", "static_field", "smoke", "toxic", "_area", "airstrike", "air_support"]) {
        "Stratagems/Strikes"
    } else {
        "Mission objects"
    }
}

/// The category of a named resource, from the name alone.
pub fn of_name(name: &str) -> String {
    let (top, rest) = name.split_once('/').unwrap_or((name, ""));
    let leaf = name.rsplit('/').next().unwrap_or(name);
    match top {
        "characters" => {
            let who = rest.split('/').next().unwrap_or(rest);
            if who.starts_with("helldiver_variation") {
                "Helldivers/Armor".into()
            } else if who.starts_with("helldiver") || who == "holodiver" {
                "Helldivers/Base".into()
            } else if who.starts_with("bug_") {
                "Enemies/Terminids".into()
            } else if who.starts_with("cyborg_") {
                "Enemies/Cyborgs".into()
            } else if who.starts_with("illuminate_") {
                "Enemies/Illuminate".into()
            } else if who.starts_with("character_") {
                "Characters".into()
            } else if who.contains("gib") {
                "Enemies/Gibs".into()
            } else {
                "Characters".into()
            }
        }
        "equipment" => {
            if let Some(stem) = rest.strip_prefix("wep_") {
                weapon(stem).into()
            } else if rest.starts_with("casing_") {
                "Weapons/Casings".into()
            } else if rest.starts_with("perk_") {
                "Weapons/Attachments".into()
            } else {
                equipment(rest.trim_start_matches("equipment_")).into()
            }
        }
        "gameobjects" => gameobject(leaf).into(),
        "gameobjects_spec" => {
            if leaf.starts_with("generic_marker") || leaf.starts_with("triggerbox") {
                "Mission objects/Markers".into()
            } else {
                "Mission objects".into()
            }
        }
        "menu" => {
            if leaf.starts_with("menu_character_model") || leaf.starts_with("menu_soldier") {
                "Helldivers/Menu".into()
            } else {
                "Menus".into()
            }
        }
        "effects" => match rest.split('/').next() {
            Some(sub) if rest.contains('/') => format!("Effects/{}", cap(sub)),
            _ => "Effects".into(),
        },
        "lua" => match rest.split_once('/') {
            Some((sub, _)) => format!("Game scripts/{}", cap(sub)),
            None => "Game scripts".into(),
        },
        t if t.starts_with("env_") => {
            let biome = cap(t.trim_start_matches("env_"));
            format!("Environment/{biome}")
        }
        "props" => "Environment/Props".into(),
        "foundation" | "core" => {
            if rest.starts_with("fonts") {
                "Fonts".into()
            } else {
                "Engine".into()
            }
        }
        "fonts" => "Fonts".into(),
        "localization" => "Text".into(),
        "intro" => "Intro".into(),
        _ if rest.is_empty() => "Other".into(),
        t => cap(t),
    }
}

/// The category of every resource, by resource id.
pub struct Categories {
    pub labels: Vec<String>,
    pub of: Vec<u16>,
}

impl Categories {
    pub fn label(&self, id: u32) -> &str {
        self.of.get(id as usize).and_then(|&i| self.labels.get(i as usize)).map(String::as_str).unwrap_or(UNNAMED)
    }

    /// Categories from names only: instant, but unnamed textures stay unnamed.
    pub fn from_names(ar: &Archive, names: &NameDb) -> Categories {
        Self::make(ar, names, None)
    }

    /// Categories from names, and textures through the materials that use them (reads every named
    /// material, a few seconds).
    pub fn build(ar: &Archive, names: &NameDb, linker: &Linker) -> Categories {
        Self::make(ar, names, Some(linker))
    }

    fn make(ar: &Archive, names: &NameDb, linker: Option<&Linker>) -> Categories {
        let mut labels: Vec<String> = vec![UNNAMED.to_string()];
        let mut index: HashMap<String, u16> = HashMap::from([(UNNAMED.to_string(), 0)]);
        let mut intern = |s: String| -> u16 {
            if let Some(&i) = index.get(&s) {
                return i;
            }
            labels.push(s.clone());
            let i = (labels.len() - 1) as u16;
            index.insert(s, i);
            i
        };
        let mut of = vec![0u16; ar.assets.len()];
        let mut by_name: HashMap<u64, u16> = HashMap::new();
        for a in &ar.assets {
            if let Some(n) = names.get(a.name_hash) {
                let c = *by_name.entry(a.name_hash).or_insert_with(|| intern(of_name(&n)));
                of[a.id as usize] = c;
            }
        }

        let Some(linker) = linker else { return Categories { labels, of } };
        // Unnamed textures: the category of the named model whose material uses them. The material of a
        // unit has the name of the unit.
        let unit = hash::hash("unit");
        let material = hash::hash("material");
        let texture = hash::hash("texture");
        let model_cats: HashMap<u64, u16> = ar.assets.iter().filter(|a| a.type_hash == unit && of[a.id as usize] != 0).map(|a| (a.name_hash, of[a.id as usize])).collect();
        let mut mats: Vec<&crate::archive::Asset> = ar.assets.iter().filter(|a| a.type_hash == material && a.patch.is_none() && model_cats.contains_key(&a.name_hash)).collect();
        mats.sort_by_key(|a| a.name_hash);
        mats.dedup_by_key(|a| a.name_hash);
        let found: Vec<(u64, u16)> = mats
            .par_iter()
            .flat_map_iter(|m| {
                let c = model_cats[&m.name_hash];
                let refs = ar.read_variant(m, 0).map(|(d, _)| linker.find(&d, "texture")).unwrap_or_default();
                refs.into_iter().map(move |r| (r.name_hash, c))
            })
            .collect();
        let mut tex_cat: HashMap<u64, u16> = HashMap::new();
        for (t, c) in found {
            // A texture shared by several models keeps the first, unless a weapon or a character wants it.
            let e = tex_cat.entry(t).or_insert(c);
            let rank = |i: u16| -> u8 {
                let l = &labels[i as usize];
                if l.starts_with("Weapons") || l.starts_with("Enemies") || l.starts_with("Helldivers") { 0 } else { 1 }
            };
            if rank(c) < rank(*e) {
                *e = c;
            }
        }
        for a in &ar.assets {
            if a.type_hash == texture && of[a.id as usize] == 0 {
                if let Some(&c) = tex_cat.get(&a.name_hash) {
                    of[a.id as usize] = c;
                }
            }
        }
        Categories { labels, of }
    }

    /// Resource counts per (kind, category), for the sidebar tree. Patch resources are left out.
    pub fn counts(&self, ar: &Archive) -> Vec<(Kind, String, usize)> {
        let mut m: HashMap<(Kind, u16), usize> = HashMap::new();
        for a in ar.assets.iter().filter(|a| a.patch.is_none()) {
            *m.entry((kind_of(a.type_hash), self.of[a.id as usize])).or_default() += 1;
        }
        let mut v: Vec<(Kind, String, usize)> = m.into_iter().map(|((k, c), n)| (k, self.labels[c as usize].clone(), n)).collect();
        v.sort_by(|a, b| (a.1 == UNNAMED, &a.1).cmp(&(b.1 == UNNAMED, &b.1)));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::of_name;

    #[test]
    fn weapons_follow_the_loadout() {
        assert_eq!(of_name("equipment/wep_assaultrifle_01"), "Weapons/Primary");
        assert_eq!(of_name("equipment/wep_railgun_01"), "Weapons/Primary");
        assert_eq!(of_name("equipment/wep_mg_02"), "Weapons/Support");
        assert_eq!(of_name("equipment/wep_mg_02_pickup"), "Weapons/Pickups");
        assert_eq!(of_name("equipment/wep_revolver_01"), "Weapons/Sidearms");
        assert_eq!(of_name("equipment/wep_ammo_backpack_01"), "Equipment/Backpacks");
        assert_eq!(of_name("equipment/equipment_impact_grenade_02"), "Equipment/Grenades");
    }

    #[test]
    fn characters_and_objects() {
        assert_eq!(of_name("characters/bug_warrior/bug_warrior"), "Enemies/Terminids");
        assert_eq!(of_name("characters/illuminate_tripod/x"), "Enemies/Illuminate");
        assert_eq!(of_name("characters/helldiver_variation/helmet_01"), "Helldivers/Armor");
        assert_eq!(of_name("gameobjects/gameobject_mechsuit_02"), "Vehicles");
        assert_eq!(of_name("gameobjects/supplycrate_law"), "Stratagems/Supply");
        assert_eq!(of_name("gameobjects/gameobject_hellpod_tesla_01"), "Stratagems/Hellpods and turrets");
        assert_eq!(of_name("env_desert/tiles/a"), "Environment/Desert");
        assert_eq!(of_name("lua/components/x"), "Game scripts/Components");
    }
}
