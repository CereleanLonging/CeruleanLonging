fn main() {
    println!("cargo:rerun-if-changed=native/DesktopObserver.cs");
    println!("cargo:rerun-if-changed=native/DesktopTrial.cs");
    println!("cargo:rerun-if-changed=native/DesktopStop.cs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let framework =
        std::path::PathBuf::from(std::env::var_os("WINDIR").expect("Windows directory"))
            .join("Microsoft.NET/Framework64/v4.0.30319");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
        .join("CeruleanDesktopObserver.exe");
    let status = std::process::Command::new(framework.join("csc.exe"))
        .args([
            "/nologo",
            "/target:exe",
            "/platform:x64",
            "/optimize+",
            "/r:System.Web.Extensions.dll",
        ])
        .arg(format!(
            "/r:{}",
            framework.join("WPF/UIAutomationClient.dll").display()
        ))
        .arg(format!(
            "/r:{}",
            framework.join("WPF/UIAutomationTypes.dll").display()
        ))
        .arg(format!(
            "/r:{}",
            framework.join("WPF/WindowsBase.dll").display()
        ))
        .arg(format!("/out:{}", output.display()))
        .arg(
            std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
                .join("native")
                .join("DesktopObserver.cs"),
        )
        .arg(
            std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
                .join("native")
                .join("DesktopTrial.cs"),
        )
        .arg(
            std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
                .join("native")
                .join("DesktopStop.cs"),
        )
        .output()
        .expect("Windows .NET Framework C# compiler required for desktop observer");
    assert!(
        status.status.success(),
        "desktop observer compilation failed: {} {}",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );
}
