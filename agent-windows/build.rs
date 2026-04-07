use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=../resources/icon/logo.ico");

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = PathBuf::from(&out_dir).join("app.rc");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let icon_path = PathBuf::from(manifest_dir)
        .parent().unwrap()
        .join("resources")
        .join("icon")
        .join("logo.ico");

    let icon_path_str = icon_path.to_str().unwrap().replace("\\", "\\\\");
    
    let rc_content = format!("app-icon ICON \"{}\"\n", icon_path_str);
    fs::write(&dest_path, rc_content).unwrap();

    embed_resource::compile(dest_path.to_str().unwrap(), embed_resource::NONE);
}
