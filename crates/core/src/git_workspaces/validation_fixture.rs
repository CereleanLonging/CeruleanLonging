#[test]
#[ignore = "child validator invoked only by approved worktree workflow tests"]
fn only_expected_isolated_inputs() {
    assert_eq!(std::fs::read("b.txt").unwrap(), b"worker result\n");
    assert_eq!(
        std::fs::read("src/a.mjs").unwrap(),
        b"export const a = 1;\n"
    );
    assert!(!std::path::Path::new(".git").exists());
}

#[test]
#[ignore = "child validator invoked only by API worktree host tests"]
fn api_worktree_inputs() {
    assert_eq!(
        std::fs::read_to_string("result.txt").unwrap(),
        "background fixture result"
    );
    assert_eq!(
        std::fs::read_to_string("base.txt").unwrap(),
        "committed base\n"
    );
    assert!(!std::path::Path::new(".git").exists());
}

#[test]
#[ignore = "child validator invoked only by OMP worktree host tests"]
fn omp_worktree_inputs() {
    assert_eq!(
        std::fs::read_to_string("result.txt").unwrap(),
        "fixture output"
    );
    assert_eq!(
        std::fs::read_to_string("base.txt").unwrap(),
        "committed base\n"
    );
    assert!(!std::path::Path::new(".git").exists());
}

#[test]
#[ignore = "child validator invoked only by Codex worktree host tests"]
fn codex_worktree_inputs() {
    assert_eq!(
        std::fs::read_to_string("sum.mjs").unwrap(),
        "export function sum(a, b) { return a + b; }\n"
    );
    assert_eq!(
        std::fs::read_to_string("base.txt").unwrap(),
        "committed base\n"
    );
    assert!(!std::path::Path::new(".git").exists());
}
