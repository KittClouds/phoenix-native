//! Read-only inspection of the same validated receipt used by Atlas Control.
use phoenix_app_core::AtlasRunReceiptV1;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("expected receipt path")?;
    let (hash, receipt) = AtlasRunReceiptV1::open_verified(std::path::Path::new(&path))?;
    println!("payload_blake3={}\n{receipt:#?}", blake3::Hash::from(hash));
    Ok(())
}
