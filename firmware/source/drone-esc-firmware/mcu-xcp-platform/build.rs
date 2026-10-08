fn main() {
    let origin=std::env::var("APP_ORIGIN").unwrap_or("0x8000".into());
    let origin=u32::from_str_radix(origin.trim_start_matches("0x"),16).expect("APP_ORIGIN hex");
    assert!(origin==0 || origin==0x8000,"approved origins only");
    let out=std::env::var("OUT_DIR").unwrap();
    std::fs::write(format!("{out}/memory.x"),format!("MEMORY {{ FLASH : ORIGIN = {origin:#x}, LENGTH = {:#x}\n RAM : ORIGIN = 0x20000000, LENGTH = 96K }}",0x3f800-origin)).unwrap();
    println!("cargo:rustc-link-search={out}");
    println!("cargo:rerun-if-env-changed=APP_ORIGIN");
}
