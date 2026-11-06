fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    // Build scripts run on the host, which may differ from the target. Embed the resource only
    // when the executable being built is for Windows.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/app-icon.ico");
        // Version information: Explorer's Details tab, Task Manager and antivirus engines read it.
        // An executable with no company, description or product name is treated with far more
        // suspicion than one that says what it is.
        res.set("ProductName", "CursorPie")
            .set("FileDescription", "CursorPie - hold-to-open pie menu")
            .set("CompanyName", "Sebastvnd")
            .set("LegalCopyright", "Copyright (c) 2026 Sebastvnd")
            .set("OriginalFilename", "cursorpie.exe")
            .set("InternalName", "cursorpie")
            .set("ProductVersion", &version)
            .set("FileVersion", &version);
        // Runs as the user (never asks for administrator rights) and declares the Windows versions
        // it was written for. DPI awareness stays with the window library, as before.
        res.set_manifest(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
      <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>
    </application>
  </compatibility>
</assembly>"#,
        );
        res.compile().expect(
            "failed to embed the icon, version info and manifest in the Windows executable",
        );
    }
}
