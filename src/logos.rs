//! Author logos, shown next to robot names. They are PNG files named after the robot's top-level
//! package, like `logos/claude.png`, and reach this directory through reviewed changes.
//!
//! The limits are checked again here so that a bad file can never reach a page.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// The largest logo file.
pub const MAX_BYTES: usize = 32 * 1024;
/// The largest width and height of a logo.
pub const MAX_SIDE: u32 = 64;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

pub type Logos = HashMap<String, Arc<Vec<u8>>>;

/// Checks that the bytes start like a PNG file within the limits, and returns its size.
pub fn check_png(bytes: &[u8]) -> Result<(u32, u32), String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("larger than {MAX_BYTES} bytes"));
    }
    // The signature comes first, then the IHDR chunk: length, type, width and height.
    if bytes.len() < 24 || bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return Err("not a PNG file".to_string());
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(format!("{width}x{height} is not within {MAX_SIDE}x{MAX_SIDE}"));
    }
    Ok((width, height))
}

/// Loads every valid logo in a directory. A missing directory just means no logos.
pub fn load_dir(directory: &Path) -> Logos {
    let mut logos = Logos::new();
    let Ok(entries) = std::fs::read_dir(directory) else { return logos };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        let Some(package) = name.strip_suffix(".png") else { continue };
        if package.is_empty() || !package.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            eprintln!("logos: ignoring {name}: the name is not a package");
            continue;
        }
        let checked = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| {
            check_png(&bytes)?;
            Ok(bytes)
        });
        match checked {
            Ok(bytes) => {
                logos.insert(package.to_string(), Arc::new(bytes));
            }
            Err(reason) => eprintln!("logos: ignoring {name}: {reason}"),
        }
    }
    logos
}

/// The package whose logo a robot uses: `claude.rm.Reckoner 0.1` uses `claude`.
pub fn package_of(robot: &str) -> &str {
    robot.split(['.', ' ']).next().unwrap_or(robot)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend([0, 0, 0, 13]);
        bytes.extend(b"IHDR");
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes
    }

    #[test]
    fn accepts_a_small_png() {
        assert_eq!(check_png(&png(16, 32)), Ok((16, 32)));
    }

    #[test]
    fn rejects_other_files_and_big_images() {
        assert!(check_png(b"<svg onload='alert(1)'></svg> and some more bytes").is_err());
        assert!(check_png(&png(65, 16)).is_err());
        assert!(check_png(&png(16, 0)).is_err());
        let mut large = png(16, 16);
        large.resize(MAX_BYTES + 1, 0);
        assert!(check_png(&large).is_err());
    }

    #[test]
    fn loads_only_valid_logos() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("good.png"), png(16, 16)).unwrap();
        std::fs::write(dir.path().join("big.png"), png(100, 100)).unwrap();
        std::fs::write(dir.path().join("not_a_logo.txt"), "text").unwrap();
        std::fs::write(dir.path().join("bad-name.png"), png(16, 16)).unwrap();
        let logos = load_dir(dir.path());
        assert_eq!(logos.keys().collect::<Vec<_>>(), ["good"]);
    }

    #[test]
    fn a_missing_directory_means_no_logos() {
        assert!(load_dir(Path::new("/no/such/directory")).is_empty());
    }

    #[test]
    fn the_package_is_the_first_part_of_the_name() {
        assert_eq!(package_of("claude.rm.Reckoner 0.1"), "claude");
        assert_eq!(package_of("nopackage 1"), "nopackage");
    }
}
