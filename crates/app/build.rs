//! The third-party license list goes into the binary deflated: 1.3 MB of JSON is 0.1 MB, and
//! only the Licenses dialog reads it (dialogs/licenses.rs).

fn main() {
    let json = "licenses/third-party-licenses.json";
    println!("cargo:rerun-if-changed={json}");
    let text = std::fs::read(json).expect(json);
    let out = std::path::Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("licenses.json.z");
    std::fs::write(out, miniz_oxide::deflate::compress_to_vec_zlib(&text, 10)).unwrap();
}
