use chrono::Datelike;
fn main() {
    let now = chrono::Utc::now().date_naive();
    println!("cargo:rustc-env=BUILD_YEAR={}", now.year());
    println!("cargo:rustc-env=BUILD_MONTH={}", now.month());
    println!("cargo:rustc-env=BUILD_DAY={}", now.day());
}
