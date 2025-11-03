//！ Builder for the whole trade engine

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // path: workdir/src/proto/oms.pb

    let base_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let proto_path = format!("{}/src/pb", base_dir);
    let out_dir = format!("{}/src/pbcode", base_dir);

    let proto_files = std::fs::read_dir(&proto_path)?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? == "proto" {
                Some(path)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let _ = tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .out_dir(out_dir)
        .compile(&proto_files, &[proto_path])?;
    Ok(())
}
