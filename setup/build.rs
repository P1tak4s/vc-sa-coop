// The installer's resources: icon, version info, an asInvoker manifest (no admin: Windows would otherwise guess
// "Setup" means "elevate"), and the mod itself as a zip (SETUP_PAYLOAD, made by tools/make_setup.py).
use std::io::Write;
use std::path::PathBuf;

include!("src/icon.rs");

fn ico(sizes: &[u32]) -> Vec<u8> {
    // ICO with 32-bit BMP (DIB) images, bottom-up, AND mask zero
    let mut images = vec![];
    for &s in sizes {
        let rgba = draw(s);
        let mut d = vec![];
        d.extend(40u32.to_le_bytes());
        d.extend((s as i32).to_le_bytes());
        d.extend((s as i32 * 2).to_le_bytes());
        d.extend(1u16.to_le_bytes());
        d.extend(32u16.to_le_bytes());
        d.extend([0u8; 24]);
        for y in (0..s).rev() {
            for x in 0..s {
                let i = ((y * s + x) * 4) as usize;
                d.extend([rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
            }
        }
        let mask_row = s.div_ceil(32) * 4;
        d.extend(vec![0u8; (mask_row * s) as usize]);
        images.push((s, d));
    }
    let mut out = vec![];
    out.extend(0u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend((images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (s, d) in &images {
        out.push(if *s >= 256 { 0 } else { *s as u8 });
        out.push(if *s >= 256 { 0 } else { *s as u8 });
        out.extend([0u8, 0u8]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((d.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += d.len() as u32;
    }
    for (_, d) in images {
        out.extend(d);
    }
    out
}

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // the mod, or an empty zip when building without it (cargo check, tests)
    println!("cargo:rerun-if-env-changed=SETUP_PAYLOAD");
    let payload = out.join("payload.zip");
    match std::env::var("SETUP_PAYLOAD") {
        Ok(p) => {
            println!("cargo:rerun-if-changed={p}");
            std::fs::copy(&p, &payload).expect("SETUP_PAYLOAD not readable");
        }
        Err(_) => {
            // smallest valid zip: an end-of-central-directory record
            let mut f = std::fs::File::create(&payload).unwrap();
            f.write_all(&[0x50, 0x4B, 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        }
    }
    let icon = out.join("setup.ico");
    std::fs::write(&icon, ico(&[16, 32, 48, 256])).unwrap();
    println!("cargo:rerun-if-changed=src/icon.rs");
    let mut res = winresource::WindowsResource::new();
    res.set_icon(icon.to_str().unwrap());
    res.set("ProductName", "VC / SA Co-op");
    res.set("FileDescription", "VC / SA Co-op diegiklis");
    res.set("CompanyName", "P1tak4s (atviras kodas)");
    res.set("LegalCopyright", "MIT licencija. Ne Rockstar Games ir ne Take-Two produktas.");
    res.set("OriginalFilename", "VC-SA-Coop-Setup.exe");
    res.set_manifest(
        r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges>
<requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security></trustInfo>
<application xmlns="urn:schemas-microsoft-com:asm.v3"><windowsSettings>
<dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
<dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
</windowsSettings></application>
<dependency><dependentAssembly><assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/></dependentAssembly></dependency>
</assembly>"#,
    );
    res.compile().expect("resources (needs the Windows SDK rc.exe)");
}
