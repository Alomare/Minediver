//! Picking resources: by text, kind, type, bundle and glob patterns over the name.

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

use crate::archive::Asset;
use crate::export::{Kind, display_name, kind_of};
use crate::hash::{self, NameDb};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Query {
    /// Words that must all appear in the name, type or hash.
    pub text: String,
    pub kinds: Vec<Kind>,
    pub types: Vec<String>,
    pub bundles: Vec<String>,
    /// Glob patterns over the name, such as `equipment/wep_*`. Any may match.
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub named_only: bool,
    /// Show resources from `.patch_N` files too.
    pub patches: bool,
    /// Categories as `kind:path`, such as `model:Weapons` or `texture:Enemies/Terminids`. A path takes in
    /// the paths under it. Any may match.
    pub cats: Vec<String>,
    /// Size limits in bytes, 0 for none.
    pub min_size: u64,
    pub max_size: u64,
}

pub struct Filter {
    cats: Vec<(Kind, String)>,
    min_size: u64,
    max_size: u64,
    words: Vec<String>,
    kinds: Vec<Kind>,
    types: Vec<u64>,
    bundles: Vec<u64>,
    include: Option<GlobSet>,
    exclude: Option<GlobSet>,
    named_only: bool,
    patches: bool,
}

fn globs(list: &[String]) -> anyhow::Result<Option<GlobSet>> {
    let list: Vec<&String> = list.iter().filter(|s| !s.trim().is_empty()).collect();
    if list.is_empty() {
        return Ok(None);
    }
    let mut b = GlobSetBuilder::new();
    for g in list {
        b.add(Glob::new(g.trim())?);
    }
    Ok(Some(b.build()?))
}

impl Filter {
    pub fn new(q: &Query) -> anyhow::Result<Filter> {
        let cats = q
            .cats
            .iter()
            .filter_map(|c| {
                let (k, p) = c.split_once(':').unwrap_or((c, ""));
                let k: Kind = serde_json::from_value(serde_json::Value::String(k.to_string())).ok()?;
                Some((k, p.to_string()))
            })
            .collect();
        Ok(Filter {
            cats,
            min_size: q.min_size,
            max_size: q.max_size,
            words: q.text.to_lowercase().split_whitespace().map(str::to_string).collect(),
            kinds: q.kinds.clone(),
            types: q.types.iter().map(|t| u64::from_str_radix(t, 16).ok().filter(|_| t.len() == 16).unwrap_or_else(|| hash::hash(t))).collect(),
            bundles: q.bundles.iter().filter_map(|b| u64::from_str_radix(b, 16).ok()).collect(),
            include: globs(&q.include)?,
            exclude: globs(&q.exclude)?,
            named_only: q.named_only,
            patches: q.patches,
        })
    }

    /// `matches`, with the category of the resource checked too.
    pub fn matches_in(&self, names: &NameDb, a: &Asset, category: &str) -> bool {
        if !self.cats.is_empty() {
            let k = kind_of(a.type_hash);
            let hit = self.cats.iter().any(|(ck, p)| *ck == k && (p.is_empty() || category == p || (category.starts_with(p.as_str()) && category.as_bytes().get(p.len()) == Some(&b'/'))));
            if !hit {
                return false;
            }
        }
        self.matches(names, a)
    }

    pub fn matches(&self, names: &NameDb, a: &Asset) -> bool {
        if a.patch.is_some() && !self.patches {
            return false;
        }
        if self.min_size > 0 || self.max_size > 0 {
            let s = a.size();
            if s < self.min_size || (self.max_size > 0 && s > self.max_size) {
                return false;
            }
        }
        if !self.kinds.is_empty() && !self.kinds.contains(&kind_of(a.type_hash)) {
            return false;
        }
        if !self.types.is_empty() && !self.types.contains(&a.type_hash) {
            return false;
        }
        if !self.bundles.is_empty() && !self.bundles.contains(&a.bundle) {
            return false;
        }
        let name = names.get(a.name_hash);
        if self.named_only && name.is_none() {
            return false;
        }
        let shown = name.clone().unwrap_or_else(|| display_name(names, a));
        if let Some(g) = &self.include {
            if !g.is_match(&shown) {
                return false;
            }
        }
        if let Some(g) = &self.exclude {
            if g.is_match(&shown) {
                return false;
            }
        }
        if !self.words.is_empty() {
            let hay = format!("{} {} {:016x} {:016x}", shown.to_lowercase(), hash::type_label(a.type_hash), a.name_hash, a.bundle);
            if !self.words.iter().all(|w| hay.contains(w.as_str())) {
                return false;
            }
        }
        true
    }
}
