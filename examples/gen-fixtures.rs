#![allow(clippy::cast_possible_truncation, clippy::many_single_char_names)]

use std::{fs, path::Path};

use arc_reader::{ImageFormat, arc::ArcVersion, pack_arc, pack_arc_audio, write};

fn make_bgi_pixels() -> (Vec<u8>, u16, u16) {
    let (w, h) = (8u16, 8u16);
    let total = usize::from(w) * usize::from(h);
    let rgba: Vec<u8> = (0..total)
        .flat_map(|i| {
            let r = ((i * 31) % 256) as u8;
            let g = ((i * 67) % 256) as u8;
            let b = ((i * 13) % 256) as u8;
            [r, g, b, 0xFF]
        })
        .collect();
    (rgba, w, h)
}

fn make_cbg_pixels() -> (Vec<u8>, u16, u16) {
    let (w, h) = (16u16, 16u16);
    let total = usize::from(w) * usize::from(h);
    let rgba: Vec<u8> = (0..total)
        .flat_map(|i| {
            let r = ((i * 41) % 256) as u8;
            let g = ((i * 73) % 256) as u8;
            let b = ((i * 17) % 256) as u8;
            let a = if (i * 11) % 256 > 128 { 0xFF } else { 0x80 };
            [r, g, b, a]
        })
        .collect();
    (rgba, w, h)
}

fn main() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures_dir = manifest_dir.join("test_assets").join("fixtures");
    fs::create_dir_all(&fixtures_dir).unwrap();
    let gen_root = manifest_dir.join("target").join("gen_fixtures");

    // 1. BGI image
    let dir = gen_root.join("bgi");
    fs::create_dir_all(&dir).unwrap();
    let (rgba, w, h) = make_bgi_pixels();
    write::write_rgba_to_png(w, h, &rgba, dir.join("image.png")).unwrap();
    let out = fixtures_dir.join("arc_bgi.arc");
    pack_arc(&dir, &out, ArcVersion::V2, ImageFormat::Bgi).unwrap();
    println!("Generated: {}", out.display());

    // 2. CBG V1 image
    let dir = gen_root.join("cbg");
    fs::create_dir_all(&dir).unwrap();
    let (rgba, w, h) = make_cbg_pixels();
    write::write_rgba_to_png(w, h, &rgba, dir.join("image.png")).unwrap();
    let out = fixtures_dir.join("arc_cbg.arc");
    pack_arc(&dir, &out, ArcVersion::V2, ImageFormat::CbgV1).unwrap();
    println!("Generated: {}", out.display());

    // 3. OGG audio
    let dir = gen_root.join("audio");
    fs::create_dir_all(&dir).unwrap();
    let ogg_data = fs::read(manifest_dir.join("test_assets").join("test.ogg")).unwrap();
    fs::write(dir.join("audio.ogg"), &ogg_data).unwrap();
    let out = fixtures_dir.join("arc_audio.arc");
    pack_arc_audio(&dir, &out, ArcVersion::V2).unwrap();
    println!("Generated: {}", out.display());

    // Cleanup temp files
    fs::remove_dir_all(&gen_root).ok();
    println!("Done! 3 fixture archives generated.");
}
