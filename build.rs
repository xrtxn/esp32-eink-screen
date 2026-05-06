use std::process::Command;
use std::{env, fs};

use vergen::{BuildBuilder, Emitter};

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let mut workspace_dir = std::path::PathBuf::from(manifest_dir);
    // if we're in a workspace, we want to be in the workspace root
    if !workspace_dir.join("web").exists() {
        workspace_dir.pop();
    }
    std::env::set_current_dir(&workspace_dir).expect("Failed to change directory to workspace");

    load_env();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    match target_arch.as_str() {
        "xtensa" => {
            // always add new git footer when files changed
            println!("cargo:rerun-if-changed=src");
            println!("cargo:rerun-if-changed=.git/HEAD");
            println!("cargo:rerun-if-changed=.git/refs");
            println!("cargo:rerun-if-changed=.git/index");

            add_git_info();
            build_index_html();
            build_display_html();
            linker_be_nice();
            // make sure linkall.x is the last linker script (otherwise might cause problems with flip-link)
            println!("cargo:rustc-link-arg=-Tlinkall.x");
        }
        "x86_64" => {
            println!("cargo:rustc-env=GIT_SHORT=emulator");
            println!("cargo:rustc-env=GIT_DIRTY=false");
            build_index_html();
            build_display_html();
        }
        _ => panic!("Unsupported target architecture: {}", target_arch),
    }
}

fn build_html(source_html: &str, favicon_path: &str, output_name: &str, env_var_name: &str) {
    println!("cargo:rerun-if-changed={source_html}");
    println!("cargo:rerun-if-changed=web/static/pico.min.css");
    println!("cargo:rerun-if-changed={favicon_path}");

    let html =
        fs::read_to_string(source_html).unwrap_or_else(|_| panic!("Failed to read {source_html}"));
    let css = fs::read_to_string("web/static/pico.min.css")
        .expect("Failed to read web/static/pico.min.css");
    let favicon =
        fs::read(favicon_path).unwrap_or_else(|_| panic!("Failed to read {favicon_path}"));

    use base64::prelude::*;
    let final_html = html
        .lines()
        .filter(|line| !line.contains(r#"link rel="stylesheet""#))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("/* CSS_PLACEHOLDER */", &css)
        .replace(
            "/* FAVICON_PLACEHOLDER */",
            &BASE64_STANDARD.encode(favicon),
        );

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let out_html = format!("{out_dir}/{output_name}");
    let out_gz = format!("{out_html}.gz");

    fs::write(&out_html, final_html).unwrap_or_else(|_| {
        panic!("Failed to write built {output_name}");
    });

    let _ = fs::remove_file(&out_gz);

    Command::new("gzip")
        .args(["-9", "-k", &out_html])
        .status()
        .unwrap_or_else(|_| panic!("Failed to gzip {output_name}"));

    let gz_len = fs::metadata(&out_gz)
        .unwrap_or_else(|_| panic!("Failed to stat gzipped {output_name}"))
        .len();

    println!("cargo:rustc-env={env_var_name}={gz_len}");
}

fn build_index_html() {
    build_html(
        "web/credentials.html",
        "web/credentials-favicon.svg",
        "credentials.html",
        "INDEX_HTML_GZ_LEN",
    );
}

fn build_display_html() {
    build_html(
        "web/calendar-config.html",
        "web/calendar-favicon.svg",
        "calendar-config.html",
        "DISPLAY_HTML_GZ_LEN",
    );
}

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                "_defmt_timestamp" => {
                    eprintln!();
                    eprintln!(
                        "💡 `defmt` not found - make sure `defmt.x` is added as a linker script and you have included `use defmt_rtt as _;`"
                    );
                    eprintln!();
                }
                "_stack_start" => {
                    eprintln!();
                    eprintln!("💡 Is the linker script `linkall.x` missing?");
                    eprintln!();
                }
                "esp_wifi_preempt_enable"
                | "esp_wifi_preempt_yield_task"
                | "esp_wifi_preempt_task_create" => {
                    eprintln!();
                    eprintln!(
                        "💡 `esp-wifi` has no scheduler enabled. Make sure you have the `builtin-scheduler` feature enabled, or that you provide an external scheduler."
                    );
                    eprintln!();
                }
                "embedded_test_linker_file_not_added_to_rustflags" => {
                    eprintln!();
                    eprintln!(
                        "💡 `embedded-test` not found - make sure `embedded-test.x` is added as a linker script for tests"
                    );
                    eprintln!();
                }
                _ => (),
            },
            // we don't have anything helpful for "missing-lib" yet
            _ => {
                std::process::exit(1);
            }
        }

        std::process::exit(0);
    }

    println!(
        "cargo:rustc-link-arg=-Wl,--error-handling-script={}",
        std::env::current_exe().unwrap().display()
    );
}

fn add_git_info() {
    // Try to get the short git hash
    let git_short = Command::new("git")
        .args(["rev-parse", "--short=8", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Check if there are uncommitted changes (dirty working tree)
    let git_dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .map(|o| {
            if o.status.success() {
                !o.stdout.is_empty()
            } else {
                false
            }
        })
        .unwrap_or(false);

    // Export as compile-time env vars accessible via `env!("GIT_SHORT")` or `option_env!("GIT_SHORT")`
    println!("cargo:rustc-env=GIT_SHORT={git_short}");
    println!("cargo:rustc-env=GIT_DIRTY={git_dirty}");

    let instructions = BuildBuilder::default()
        .build_timestamp(true)
        .build()
        .unwrap();

    Emitter::default()
        .add_instructions(&instructions)
        .unwrap()
        .emit()
        .unwrap();
}

fn load_env() {
    dotenvy::dotenv().ok();

    println!("cargo:rerun-if-changed=.env");

    if let Ok(val) = env::var("AP_SSID") {
        println!("cargo:rustc-env=AP_SSID={}", val);
    }
    if let Ok(val) = env::var("AP_PASS") {
        println!("cargo:rustc-env=AP_PASS={}", val);
    }
}
