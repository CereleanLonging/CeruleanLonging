//! Read original team observation; never opens RuntimeHost or dispatches work.
#[cfg(windows)]
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = if args.len() == 3 {
        cerulean_core::host::team_observation::inspect(
            std::path::Path::new(&args[0]),
            &args[1],
            &args[2],
        )
    } else {
        Err("usage: inspect_team_run <data-root> <project> <start-digest>".into())
    };
    match result {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1)
        }
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows team observation required");
    std::process::exit(1)
}
