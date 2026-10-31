// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the Worldkiln Source-Available License.
// See LICENSE in the repository root for the complete terms.

fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads/main");
    let head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "unavailable".into());
    println!("cargo:rustc-env=WORLDKILN_SOURCE_COMMIT={head}");

    println!("cargo:rerun-if-changed=assets/branding/worldkiln-emblem.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let root = std::path::PathBuf::from(
            std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"),
        );
        let icon = root.join("assets/branding/worldkiln-emblem.ico");
        if icon.is_file() {
            let resource =
                std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("build output"))
                    .join("worldkiln.rc");
            let icon = icon.to_string_lossy().replace('\\', "/");
            std::fs::write(&resource, format!("1 ICON \"{icon}\"\n"))
                .expect("write Worldkiln Windows resource description");
            embed_resource::compile(&resource, embed_resource::NONE)
                .manifest_optional()
                .expect("failed to compile Worldkiln Windows resources");
        }
    }
}
