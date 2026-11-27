//! Compiles the shaders (WGSL) to SPIR-V with naga, so building needs no outside tools.

fn main() {
    for name in ["quads"] {
        let path = format!("shaders/{name}.wgsl");
        println!("cargo:rerun-if-changed={path}");
        let src = std::fs::read_to_string(&path).unwrap();
        let module = naga::front::wgsl::parse_str(&src)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{path}: {e:?}"));
        let opts = naga::back::spv::Options {
            lang_version: (1, 3),
            ..Default::default()
        };
        let words = naga::back::spv::write_vec(&module, &info, &opts, None).unwrap();
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let out =
            std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join(format!("{name}.spv"));
        std::fs::write(out, bytes).unwrap();
    }
}
