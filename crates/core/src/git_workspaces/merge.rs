//! File-level DAG composition. Writer ancestry, not traversal order, resolves
//! inherited edits. Incomparable writers with different values are conflicts.
use crate::source_snapshot::Tree;
use cerulean_domain::{execution::*, workflow::WorkflowTask};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    Directory,
    File(Artifact),
}
#[derive(Clone)]
struct Cell {
    value: Option<Entry>,
    writers: BTreeSet<String>,
}
type State = BTreeMap<String, Cell>;
pub(super) struct InputTree {
    pub task: String,
    pub before: Tree,
    pub after: Tree,
}
fn entries(tree: &Tree) -> Result<BTreeMap<String, Entry>, String> {
    let mut result = BTreeMap::new();
    let mut seen = BTreeSet::new();
    require(
        tree.files.len() <= 4096 && tree.directories.len() <= 20000,
        "merged input entry limit",
    )?;
    let mut total = 0u64;
    for (name, value) in tree
        .directories
        .iter()
        .map(|p| (p, Entry::Directory))
        .chain(tree.files.iter().map(|f| (&f.path, Entry::File(f.clone()))))
    {
        artifact_path(name)?;
        require(
            name.split('/').count() <= 64
                && !name.split('/').any(|p| p.eq_ignore_ascii_case(".git")),
            "invalid merge path",
        )?;
        require(
            seen.insert(name.to_lowercase()),
            "merged input path/case collision",
        )?;
        if let Entry::File(file) = &value {
            sha256(&file.sha256)?;
            total += u64::from(file.bytes);
            require(
                file.bytes <= 1024 * 1024 && total <= 256 * 1024 * 1024,
                "merged input byte limit",
            )?;
        }
        result.insert(name.clone(), value);
    }
    for name in result.keys() {
        if let Some((parent, _)) = name.rsplit_once('/') {
            require(
                result.get(parent) == Some(&Entry::Directory),
                "merged input file/directory conflict",
            )?;
        }
    }
    Ok(result)
}
fn tree(state: &State) -> Tree {
    let mut result = Tree {
        directories: vec![],
        files: vec![],
    };
    for (path, cell) in state {
        match &cell.value {
            Some(Entry::Directory) => result.directories.push(path.clone()),
            Some(Entry::File(file)) => result.files.push(file.clone()),
            None => {}
        }
    }
    result
}
fn combine(
    base: &State,
    parents: &[&State],
    ancestry: &BTreeMap<String, BTreeSet<String>>,
) -> Result<State, String> {
    if parents.is_empty() {
        return Ok(base.clone());
    }
    let keys: BTreeSet<_> = parents
        .iter()
        .flat_map(|p| p.keys().cloned())
        .chain(base.keys().cloned())
        .collect();
    let mut result = State::new();
    for path in keys {
        let mut writers = BTreeMap::new();
        for parent in parents {
            if let Some(cell) = parent.get(&path) {
                for id in &cell.writers {
                    if let Some(old) = writers.insert(id.clone(), cell.value.clone()) {
                        require(
                            old == cell.value,
                            "same writer has conflicting input values",
                        )?;
                    }
                }
            }
        }
        let all: Vec<_> = writers.keys().cloned().collect();
        writers.retain(|id, _| {
            !all.iter()
                .any(|other| ancestry.get(other).is_some_and(|a| a.contains(id)))
        });
        let cell = if let Some(value) = writers.values().next().cloned() {
            require(
                writers.values().all(|v| *v == value),
                &format!("dependency input conflict at {path}"),
            )?;
            Cell {
                value,
                writers: writers.keys().cloned().collect(),
            }
        } else {
            base.get(&path).cloned().unwrap_or(Cell {
                value: None,
                writers: BTreeSet::new(),
            })
        };
        result.insert(path, cell);
    }
    entries(&tree(&result))?;
    Ok(result)
}
struct Composer<'a> {
    definitions: BTreeMap<&'a str, &'a WorkflowTask>,
    inputs: BTreeMap<&'a str, &'a InputTree>,
    states: BTreeMap<String, State>,
    ancestry: BTreeMap<String, BTreeSet<String>>,
    visiting: BTreeSet<String>,
    base: State,
}
impl Composer<'_> {
    fn visit(&mut self, id: &str) -> Result<(), String> {
        if self.states.contains_key(id) {
            return Ok(());
        }
        require(self.visiting.insert(id.into()), "dependency cycle")?;
        let def = *self
            .definitions
            .get(id)
            .ok_or("dependency definition missing")?;
        let input = *self
            .inputs
            .get(id)
            .ok_or("accepted dependency tree missing")?;
        let mut ancestors = BTreeSet::new();
        for parent in &def.dependencies {
            self.visit(parent)?;
            ancestors.insert(parent.clone());
            ancestors.extend(self.ancestry[parent].iter().cloned());
        }
        let parents: Vec<_> = def.dependencies.iter().map(|p| &self.states[p]).collect();
        let mut state = combine(&self.base, &parents, &self.ancestry)?;
        let before = entries(&input.before)?;
        require(
            entries(&tree(&state))? == before,
            "accepted task initial tree differs from its dependency inputs",
        )?;
        let after = entries(&input.after)?;
        let keys: BTreeSet<_> = before.keys().chain(after.keys()).cloned().collect();
        for path in keys {
            if before.get(&path) != after.get(&path) {
                state.insert(
                    path.clone(),
                    Cell {
                        value: after.get(&path).cloned(),
                        writers: [id.to_owned()].into(),
                    },
                );
            }
        }
        self.states.insert(id.into(), state);
        self.ancestry.insert(id.into(), ancestors);
        self.visiting.remove(id);
        Ok(())
    }
}
pub(super) fn compose(
    definitions: &[WorkflowTask],
    target: &str,
    base: &Tree,
    inputs: &[InputTree],
) -> Result<Tree, String> {
    let base = entries(base)?
        .into_iter()
        .map(|(path, value)| {
            (
                path,
                Cell {
                    value: Some(value),
                    writers: BTreeSet::new(),
                },
            )
        })
        .collect();
    let mut composer = Composer {
        definitions: definitions.iter().map(|t| (t.id.as_str(), t)).collect(),
        inputs: inputs.iter().map(|t| (t.task.as_str(), t)).collect(),
        states: BTreeMap::new(),
        ancestry: BTreeMap::new(),
        visiting: BTreeSet::new(),
        base,
    };
    require(
        composer.definitions.len() == definitions.len() && composer.inputs.len() == inputs.len(),
        "duplicate merge task",
    )?;
    let target = *composer
        .definitions
        .get(target)
        .ok_or("merge target missing")?;
    for parent in &target.dependencies {
        composer.visit(parent)?;
    }
    require(
        composer.states.len() == inputs.len(),
        "unrelated dependency input",
    )?;
    let parents: Vec<_> = target
        .dependencies
        .iter()
        .map(|p| &composer.states[p])
        .collect();
    let merged = combine(&composer.base, &parents, &composer.ancestry)?;
    Ok(tree(&merged))
}

pub(super) fn validate_tree(tree: &Tree) -> Result<(), String> {
    entries(tree).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn t(values: &[(&str, u8)]) -> Tree {
        Tree {
            directories: vec![],
            files: values
                .iter()
                .map(|(p, n)| Artifact {
                    path: (*p).into(),
                    bytes: 1,
                    sha256: format!("{n:02x}").repeat(32),
                })
                .collect(),
        }
    }
    fn task(id: &str, parents: &[&str]) -> WorkflowTask {
        WorkflowTask {
            id: id.into(),
            goal: id.into(),
            workspace_key: "p".into(),
            dependencies: parents.iter().map(|s| (*s).into()).collect(),
            resource_keys: None,
        }
    }
    fn input(id: &str, before: &Tree, after: &Tree) -> InputTree {
        InputTree {
            task: id.into(),
            before: before.clone(),
            after: after.clone(),
        }
    }
    #[test]
    fn diamond_add_modify_delete_and_shared_ancestor() {
        let base = t(&[("base", 0)]);
        let a = t(&[("base", 0), ("added", 1)]);
        let b = t(&[("base", 2)]);
        let c = t(&[("base", 0), ("added", 1), ("other", 3)]);
        let defs = vec![
            task("a", &[]),
            task("b", &["a"]),
            task("c", &["a"]),
            task("end", &["b", "c"]),
        ];
        let inputs = vec![
            input("a", &base, &a),
            input("b", &a, &b),
            input("c", &a, &c),
        ];
        assert_eq!(
            compose(&defs, "end", &base, &inputs).unwrap(),
            t(&[("base", 2), ("other", 3)])
        );
    }
    #[test]
    fn equal_independent_edits_then_descendant_diverges_always_conflict() {
        let base = t(&[("f", 0)]);
        let one = t(&[("f", 1)]);
        let two = t(&[("f", 2)]);
        for parents in [["b", "c"], ["c", "b"]] {
            let defs = vec![
                task("a", &[]),
                task("b", &[]),
                task("c", &["a"]),
                task("end", &parents),
            ];
            let inputs = vec![
                input("a", &base, &one),
                input("b", &base, &one),
                input("c", &one, &two),
            ];
            assert!(compose(&defs, "end", &base, &inputs)
                .unwrap_err()
                .contains("conflict at f"));
        }
    }
    #[test]
    fn identical_edits_coalesce_but_initial_tree_cannot_lie() {
        let base = t(&[("f", 0)]);
        let one = t(&[("f", 1)]);
        let defs = vec![task("a", &[]), task("b", &[]), task("end", &["a", "b"])];
        assert_eq!(
            compose(
                &defs,
                "end",
                &base,
                &[input("a", &base, &one), input("b", &base, &one)]
            )
            .unwrap(),
            one
        );
        assert!(compose(
            &defs,
            "end",
            &base,
            &[input("a", &one, &one), input("b", &base, &one)]
        )
        .is_err());
        assert!(validate_tree(&t(&[("F", 0), ("f", 1)])).is_err());
        assert!(validate_tree(&t(&[("f", 0), ("f/nested", 1)])).is_err());
    }
}
