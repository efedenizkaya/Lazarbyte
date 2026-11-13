//! Binding generator, per the uniffi multi-crate workspace recipe:
//! `cargo run -p uniffi-bindgen generate <library> --language kotlin --out-dir out`

fn main() {
    uniffi::uniffi_bindgen_main()
}
