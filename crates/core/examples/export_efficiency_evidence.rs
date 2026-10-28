//! Offline, read-only exporter. It cannot execute models or grant approval.
#[cfg(windows)]
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let result = (|| -> Result<(), String> {
        if args.len() != 4 && !(args.len() == 5 && args[4] == "--include-management") {
            return Err("usage: export_efficiency_evidence <data-root> <project> <revision-digest> <ledger-head> [--include-management]".into());
        }
        let text = |i: usize| args[i].to_str().ok_or("argument is not Unicode".to_owned());
        let reader = if args.len() == 5 {
            cerulean_core::efficiency_evidence::read_with_management
        } else {
            cerulean_core::efficiency_evidence::read
        };
        let evidence = reader(std::path::Path::new(&args[0]), text(1)?, text(2)?, text(3)?)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&evidence).map_err(|e| e.to_string())?
        );
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Original Windows workflow evidence required");
    std::process::exit(1);
}
