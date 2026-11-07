fn csharp_compiler() -> std::path::PathBuf {
    let installer = std::path::PathBuf::from(
        std::env::var_os("ProgramFiles(x86)").expect("Windows program directory"),
    )
    .join("Microsoft Visual Studio/Installer/vswhere.exe");
    let located = std::process::Command::new(installer)
        .args([
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.Component.MSBuild",
            "-find",
            "MSBuild/Current/Bin/Roslyn/csc.exe",
            "-utf8",
        ])
        .output()
        .expect("Visual Studio Build Tools locator required for desktop observer");
    assert!(located.status.success(), "C# compiler discovery failed");
    let text = String::from_utf8(located.stdout).expect("C# compiler path must be UTF-8");
    let paths: Vec<_> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(
        paths.len(),
        1,
        "A single Visual Studio Roslyn compiler is required"
    );
    let compiler = std::path::PathBuf::from(paths[0].trim());
    assert!(
        compiler.is_absolute() && compiler.is_file(),
        "C# compiler is unavailable"
    );
    compiler
}

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
    let source = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let compiler = csharp_compiler();
    println!("cargo:rerun-if-changed={}", compiler.display());
    let status = std::process::Command::new(compiler)
        .args([
            "/nologo",
            "/noconfig",
            "/nostdlib+",
            "/target:exe",
            "/platform:x64",
            "/optimize+",
            "/debug-",
            "/langversion:5",
            "/deterministic+",
            "/utf8output",
        ])
        .arg(format!("/pathmap:{}=/_/cerulean-core", source.display()))
        .args(
            [
                "mscorlib.dll",
                "System.dll",
                "System.Core.dll",
                "System.Web.Extensions.dll",
            ]
            .map(|name| format!("/r:{}", framework.join(name).display())),
        )
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
        .arg(source.join("native").join("DesktopObserver.cs"))
        .arg(source.join("native").join("DesktopTrial.cs"))
        .arg(source.join("native").join("DesktopStop.cs"))
        .output()
        .expect("Visual Studio Roslyn C# compiler required for desktop observer");
    assert!(
        status.status.success(),
        "desktop observer compilation failed: {} {}",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );
}
