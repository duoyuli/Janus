//! How the library's Skill tab lists the skills: in the router's catalog order (the most read
//! first, then by name), and folded by where they came from (each GitHub source in the order the
//! sources are listed, then each local folder they were copied out of, then the ones written in
//! Janus). Worked out here for every client.
// @project-doc docs/domains/20261116-resource_library.md#skill_router

use super::{Item, Origin};
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// The router's catalog order: the most read first (`uses`, sessions), then by name.
pub(super) fn by_use(a: (u64, &str), b: (u64, &str)) -> Ordering {
    b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1))
}

/// Skills that came from the same place.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SkillGroup {
    /// A GitHub source, by its id.
    Source { source: String, items: Vec<String> },
    /// A local folder the skills were copied out of.
    Folder { path: String, items: Vec<String> },
    /// Written in Janus.
    Created { items: Vec<String> },
}

/// `skills` by id in catalog order, given each one's sessions in `uses`.
pub(super) fn catalog_order(skills: &[&Item], uses: &HashMap<String, u64>) -> Vec<String> {
    let mut list: Vec<&&Item> = skills.iter().collect();
    let used = |i: &Item| uses.get(&i.id).copied().unwrap_or(0);
    list.sort_by(|a, b| by_use((used(a), &a.name), (used(b), &b.name)));
    list.into_iter().map(|i| i.id.clone()).collect()
}

/// `skills` by where they came from; `sources` are the GitHub sources' ids in their order (each
/// gets a group, empty until a skill is imported from it).
pub(super) fn groups(skills: &[&Item], sources: &[String]) -> Vec<SkillGroup> {
    let mut by_source: Vec<(String, Vec<String>)> = sources.iter().map(|s| (s.clone(), vec![])).collect();
    let mut folders: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut created = vec![];
    for s in skills {
        match &s.origin {
            Origin::Git { source, .. } => match by_source.iter_mut().find(|(id, _)| id == source) {
                Some((_, items)) => items.push(s.id.clone()),
                None => by_source.push((source.clone(), vec![s.id.clone()])),
            },
            Origin::Folder { path } => {
                let dir = Path::new(path).parent().map(|p| p.display().to_string()).unwrap_or_default();
                folders.entry(dir).or_default().push(s.id.clone());
            }
            Origin::Created => created.push(s.id.clone()),
        }
    }
    let mut out: Vec<SkillGroup> =
        by_source.into_iter().map(|(source, items)| SkillGroup::Source { source, items }).collect();
    out.extend(folders.into_iter().map(|(path, items)| SkillGroup::Folder { path, items }));
    if !created.is_empty() {
        out.push(SkillGroup::Created { items: created });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Body;

    fn skill(id: &str, origin: Origin) -> Item {
        Item {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            origin,
            revision: String::new(),
            updated_at: 0,
            body: Body::Skill,
        }
    }

    #[test]
    fn skills_are_listed_by_use_then_name_and_grouped_by_where_they_came_from() {
        let items = [
            skill("b", Origin::Created),
            skill("a", Origin::Folder { path: "/x/kit/a".into() }),
            skill("c", Origin::Git { source: "s2".into(), path: "c".into() }),
            skill("d", Origin::Git { source: "s9".into(), path: "d".into() }),
            skill("e", Origin::Folder { path: "/a/e".into() }),
        ];
        let refs: Vec<&Item> = items.iter().collect();
        let uses = HashMap::from([("c".to_string(), 3), ("e".to_string(), 3)]);
        assert_eq!(catalog_order(&refs, &uses), ["c", "e", "a", "b", "d"]);
        let g = groups(&refs, &["s1".into(), "s2".into()]);
        assert_eq!(
            g,
            [
                SkillGroup::Source { source: "s1".into(), items: vec![] },
                SkillGroup::Source { source: "s2".into(), items: vec!["c".into()] },
                SkillGroup::Source { source: "s9".into(), items: vec!["d".into()] },
                SkillGroup::Folder { path: "/a".into(), items: vec!["e".into()] },
                SkillGroup::Folder { path: "/x/kit".into(), items: vec!["a".into()] },
                SkillGroup::Created { items: vec!["b".into()] },
            ]
        );
    }
}
