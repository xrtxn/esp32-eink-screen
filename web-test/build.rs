use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../web/credentials.html");
    println!("cargo:rerun-if-changed=../web/calendar-config.html");
    println!("cargo:rerun-if-changed=../web/static/pico.min.css");
    build_index_html();
    build_display_html();
}

fn build_index_html() {
    let html = std::fs::read_to_string("../web/credentials.html").expect("Failed to read credentials.html");
    let css =
        std::fs::read_to_string("../web/static/pico.min.css").expect("Failed to read pico.min.css");
    let favicon = std::fs::read("../web/cog.svg").expect("Failed to read");

    use base64::prelude::*;
    let favicon_base64 = BASE64_STANDARD.encode(&favicon);

    let final_html = html
        .lines()
        .filter(|line| !line.contains(r#"link rel="stylesheet""#))
        .collect::<Vec<&str>>()
        .join("\n")
        .replace("/* CSS_PLACEHOLDER */", &css)
        .replace("/* FAVICON_PLACEHOLDER */", &favicon_base64);

    let out_dir = env::var("OUT_DIR").unwrap();
    let out_html = format!("{}/credentials.html", out_dir);
    let out_gz = format!("{}/credentials.html.gz", out_dir);

    std::fs::write(&out_html, final_html.as_bytes()).expect("Failed to write built credentials.html");

    let _ = std::fs::remove_file(&out_gz);

    Command::new("gzip")
        .args(["-9", "-k", &out_html])
        .status()
        .expect("Failed to gzip credentials.html");

    let gz_len = std::fs::metadata(&out_gz)
        .expect("Failed to stat gzipped credentials.html")
        .len();
    println!("cargo:rustc-env=INDEX_HTML_GZ_LEN={}", gz_len);
}

fn build_display_html() {
    let html =
        std::fs::read_to_string("../web/calendar-config.html").expect("Failed to read web/calendar-config.html");
    let css = std::fs::read_to_string("../web/static/pico.min.css")
        .expect("Failed to read web/static/pico.min.css");
    let favicon = std::fs::read("../web/calendar-cog.svg").expect("Failed to read");

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

    std::fs::write(&out_html, final_html.as_bytes()).expect("Failed to write built calendar-config.html");

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
