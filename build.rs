use std::fmt::Write as _;
use std::path::PathBuf;
use std::{env, fs};

fn main() {
    // Debug builds do not inline GPUI element builders. Opening 模型渠道 builds
    // that tree from a click handler, which overflows the default 1MB main-thread
    // stack (`thread 'main' has overflowed its stack`).
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg=/STACK:8388608");
    }
    embed_brand_icons();
}

/// 把 assets/brand 下的品牌 SVG 嵌入程序，生成 `BRAND_ICONS` 列表。
///
/// LobeHub 的图标把宽高写成 1em，GPUI 按 12px 栅格化后放大会发虚，这里统一改成 64。
fn embed_brand_icons() {
    println!("cargo:rerun-if-changed=assets/brand");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let target = out_dir.join("brand");
    fs::create_dir_all(&target).expect("create brand output dir");

    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir("assets/brand") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("svg") {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()).map(str::to_string) else {
                continue;
            };
            let Ok(svg) = fs::read_to_string(&path) else {
                continue;
            };
            let svg = svg
                .replacen("width=\"1em\"", "width=\"64\"", 1)
                .replacen("height=\"1em\"", "height=\"64\"", 1);
            fs::write(target.join(&name), svg).expect("write brand icon");
            names.push(name);
        }
    }
    names.sort();

    let mut code = String::from("/// 构建时从 assets/brand 嵌入的品牌图标：(文件名, SVG 内容)\n");
    code.push_str("pub static BRAND_ICONS: &[(&str, &[u8])] = &[\n");
    for name in &names {
        writeln!(code, "    ({name:?}, include_bytes!(concat!(env!(\"OUT_DIR\"), \"/brand/{name}\"))),").unwrap();
    }
    code.push_str("];\n");
    fs::write(out_dir.join("brand_icons.rs"), code).expect("write brand_icons.rs");
}
