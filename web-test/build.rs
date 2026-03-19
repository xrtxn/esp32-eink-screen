use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../web/index.html");
    println!("cargo:rerun-if-changed=../web/config.html");
    println!("cargo:rerun-if-changed=../web/static/pico.min.css");
    build_index_html();
    build_display_html();
}

fn build_index_html() {
    let html = std::fs::read_to_string("../web/index.html").expect("Failed to read index.html");
    let css =
        std::fs::read_to_string("../web/static/pico.min.css").expect("Failed to read pico.min.css");

    let final_html = html
        .lines()
        .filter(|line| !line.contains(r#"link rel="stylesheet""#))
        .collect::<Vec<&str>>()
        .join("\n")
        .replace("/* CSS_PLACEHOLDER */", &css);

    let out_dir = env::var("OUT_DIR").unwrap();
    let out_html = format!("{}/index.html", out_dir);
    let out_gz = format!("{}/index.html.gz", out_dir);

    std::fs::write(&out_html, final_html.as_bytes()).expect("Failed to write built index.html");

    let _ = std::fs::remove_file(&out_gz);

    Command::new("gzip")
        .args(["-9", "-k", &out_html])
        .status()
        .expect("Failed to gzip index.html");

    let gz_len = std::fs::metadata(&out_gz)
        .expect("Failed to stat gzipped index.html")
        .len();
    println!("cargo:rustc-env=INDEX_HTML_GZ_LEN={}", gz_len);
}

fn build_display_html() {
    let html = std::fs::read_to_string("../web/config.html").expect("Failed to read web/config.html");
    let css = std::fs::read_to_string("../web/static/pico.min.css")
        .expect("Failed to read web/static/pico.min.css");

    // Replace the placeholder that was used by the Askama template
    // Remove css link which is for local development
    let final_html = html
        .lines()
        .filter(|line| !line.contains(r#"link rel="stylesheet""#))
        .collect::<Vec<&str>>()
        .join("\n")
        .replace("/* CSS_PLACEHOLDER */", &css);

    let out_dir = env::var("OUT_DIR").unwrap();
    let out_html = format!("{out_dir}/config.html");
    let out_gz = format!("{out_dir}/config.html.gz");

    std::fs::write(&out_html, final_html.as_bytes()).expect("Failed to write built config.html");

    let _ = std::fs::remove_file(&out_gz);

    Command::new("gzip")
        .args(["-9", "-k", &out_html])
        .status()
        .expect("Failed to gzip config.html — is gzip installed?");

    let gz_len = std::fs::metadata(&out_gz)
        .expect("Failed to stat gzipped config.html")
        .len();
    println!("cargo:rustc-env=DISPLAY_HTML_GZ_LEN={gz_len}");
}
