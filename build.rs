use std::env;
use std::process::Command;

use vergen::{BuildBuilder, Emitter};

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let mut workspace_dir = std::path::PathBuf::from(manifest_dir);
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

fn build_index_html() {
    println!("cargo:rerun-if-changed=web/credentials.html");
    println!("cargo:rerun-if-changed=web/static/pico.min.css");
    println!("cargo:rerun-if-changed=web/credentials-favicon.svg");

    let html = std::fs::read_to_string("web/credentials.html")
        .expect("Failed to read web/credentials.html");
    let css = std::fs::read_to_string("web/static/pico.min.css")
        .expect("Failed to read web/static/pico.min.css");
    let favicon = std::fs::read("web/credentials-favicon.svg").expect("Failed to read");

    use base64::prelude::*;
    let favicon_base64 = BASE64_STANDARD.encode(&favicon);

    // Replace the placeholder that was used by the Askama template
    // Remove css link which is for local development
    let final_html = html
        .lines()
        .filter(|line| !line.contains(r#"link rel="stylesheet""#))
        .collect::<Vec<&str>>()
        .join("\n")
        .replace("/* CSS_PLACEHOLDER */", &css)
        .replace("/* FAVICON_PLACEHOLDER */", &favicon_base64);

    let out_dir = env::var("OUT_DIR").unwrap();
    let out_html = format!("{out_dir}/credentials.html");
    let out_gz = format!("{out_dir}/credentials.html.gz");

    std::fs::write(&out_html, final_html.as_bytes())
        .expect("Failed to write built credentials.html");

    let _ = std::fs::remove_file(&out_gz);

    Command::new("gzip")
        .args(["-9", "-k", &out_html])
        .status()
        .expect("Failed to gzip credentials.html — is gzip installed?");

    let gz_len = std::fs::metadata(&out_gz)
        .expect("Failed to stat gzipped credentials.html")
        .len();
    println!("cargo:rustc-env=INDEX_HTML_GZ_LEN={gz_len}");
}

fn build_display_html() {
    println!("cargo:rerun-if-changed=web/calendar-config.html");
    println!("cargo:rerun-if-changed=web/static/pico.min.css");
    println!("cargo:rerun-if-changed=web/calendar-favicon.svg");

    let html = std::fs::read_to_string("web/calendar-config.html")
        .expect("Failed to read web/calendar-config.html");
    let css = std::fs::read_to_string("web/static/pico.min.css")
        .expect("Failed to read web/static/pico.min.css");
    let favicon = std::fs::read("web/calendar-favicon.svg").expect("Failed to read favicon");

    use base64::prelude::*;
    let favicon_base64 = BASE64_STANDARD.encode(&favicon);

    // Replace the placeholder that was used by the Askama template
    // Remove css link which is for local development
    let final_html = html
        .lines()
        .filter(|line| !line.contains(r#"link rel="stylesheet""#))
        .collect::<Vec<&str>>()
        .join("\n")
        .replace("/* CSS_PLACEHOLDER */", &css)
        .replace("/* FAVICON_PLACEHOLDER */", &favicon_base64);

    let out_dir = env::var("OUT_DIR").unwrap();
    let out_html = format!("{out_dir}/calendar-config.html");
    let out_gz = format!("{out_dir}/calendar-config.html.gz");

    std::fs::write(&out_html, final_html.as_bytes())
        .expect("Failed to write built calendar-config.html");

    let _ = std::fs::remove_file(&out_gz);

    Command::new("gzip")
        .args(["-9", "-k", &out_html])
        .status()
        .expect("Failed to gzip calendar-config.html — is gzip installed?");

    let gz_len = std::fs::metadata(&out_gz)
        .expect("Failed to stat gzipped calendar-config.html")
        .len();
    println!("cargo:rustc-env=DISPLAY_HTML_GZ_LEN={gz_len}");
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

    if let Ok(val) = env::var("WIFI_SSID") {
        println!("cargo:rustc-env=WIFI_SSID={}", val);
    }
    if let Ok(val) = env::var("WIFI_PASS") {
        println!("cargo:rustc-env=WIFI_PASS={}", val);
    }
    if let Ok(val) = env::var("ORIGIN") {
        println!("cargo:rustc-env=ORIGIN={}", val);
    }
    if let Ok(val) = env::var("CALDAV_USER") {
        println!("cargo:rustc-env=CALDAV_USER={}", val);
    }
    if let Ok(val) = env::var("CALDAV_PASS") {
        println!("cargo:rustc-env=CALDAV_PASS={}", val);
    }
    if let Ok(val) = env::var("AP_SSID") {
        println!("cargo:rustc-env=AP_SSID={}", val);
    }
    if let Ok(val) = env::var("AP_PASS") {
        println!("cargo:rustc-env=AP_PASS={}", val);
    }
}
