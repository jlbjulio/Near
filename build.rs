use std::{env, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=assets/near.ico");
    println!("cargo:rerun-if-changed=app.manifest");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let rc = out.join("near.rc");
    let res = out.join("near.res");
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let numeric_version = format!(
        "{},{},{},0",
        env::var("CARGO_PKG_VERSION_MAJOR").unwrap(),
        env::var("CARGO_PKG_VERSION_MINOR").unwrap(),
        env::var("CARGO_PKG_VERSION_PATCH").unwrap()
    );
    std::fs::write(&rc, format!("1 ICON \"{}\"\n1 24 \"{}\"\n1 VERSIONINFO\nFILEVERSION {numeric_version}\nPRODUCTVERSION {numeric_version}\nFILETYPE 1\nBEGIN\n BLOCK \"StringFileInfo\"\n BEGIN\n BLOCK \"040904b0\"\n BEGIN\n VALUE \"FileDescription\", \"Near - AirPods for Windows\"\n VALUE \"ProductName\", \"Near\"\n VALUE \"FileVersion\", \"{version}\"\n VALUE \"ProductVersion\", \"{version}\"\n END\n END\n BLOCK \"VarFileInfo\"\n BEGIN\n VALUE \"Translation\", 0x409, 1200\n END\nEND", root.join("assets/near.ico").display().to_string().replace('\\',"\\\\"), root.join("app.manifest").display().to_string().replace('\\',"\\\\"))).unwrap();
    let status = Command::new("rc.exe")
        .arg("/nologo")
        .arg(format!("/fo{}", res.display()))
        .arg(&rc)
        .status()
        .expect("Windows SDK rc.exe must be on PATH");
    assert!(status.success(), "Resource compilation failed");
    println!("cargo:rustc-link-arg={}", res.display());
}
