use super::catalog;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

thread_local! {
    static FAULT: RefCell<Option<(PathBuf, &'static str)>> = const { RefCell::new(None) };
}

// Thread-scoped, test-only injection creates a genuine filesystem conflict.
pub(super) fn fault(folder: &Path, phase: &str) {
    FAULT.with(|selected| {
        let mut selected = selected.borrow_mut();
        if selected
            .as_ref()
            .is_some_and(|(path, at)| path == folder && *at == phase)
        {
            fs::create_dir(folder.join(if phase == "after-intent" {
                "data"
            } else {
                "ready.json"
            }))
            .unwrap();
            *selected = None;
        }
    });
}

fn tree(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            result.insert(path.clone(), None);
            result.extend(tree(&path));
        } else {
            result.insert(path.clone(), Some(fs::read(path).unwrap()));
        }
    }
    result
}

#[test]
fn real_creation_failures_preserve_original_and_allow_only_new_identifiers() {
    for (phase, expected) in [
        ("after-intent", "创建尚未确认完成"),
        ("before-publication", "创建发布未完成"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let catalog = temp.path().join("catalog");
        fs::create_dir(&catalog).unwrap();
        let catalog = fs::canonicalize(catalog).unwrap();
        let original = catalog.join("exp-failed");
        FAULT.with(|slot| *slot.borrow_mut() = Some((original.clone(), phase)));
        assert!(catalog::create(&catalog, "exp-failed").is_err());
        FAULT.with(|slot| {
            assert!(
                slot.borrow().is_none(),
                "Fault must occur in the original creator"
            )
        });
        assert!(original.join("intent.json").is_file());
        if phase == "before-publication" {
            assert!(original.join("ready.pending").is_file());
            assert!(original.join("ready.json").is_dir());
        }
        let before = tree(&original);
        for _ in 0..2 {
            let entries = catalog::list(&catalog).unwrap();
            assert_eq!(entries.len(), 1);
            assert!(!entries[0].ready);
            assert!(entries[0].error.as_ref().unwrap().contains(expected));
            assert!(catalog::open(&catalog, "exp-failed").is_err());
            assert!(catalog::create(&catalog, "exp-failed").is_err());
            assert_eq!(before, tree(&original));
        }
        assert!(catalog::create(&catalog, "exp-new").unwrap().ready);
        assert_eq!(before, tree(&original));
        assert_eq!(catalog::list(&catalog).unwrap().len(), 2);
    }
}
