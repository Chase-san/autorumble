//! The flags and logos shown next to robot names.
//!
//! `logos/` holds the images as PNG files, which reach it through reviewed changes. A list says which
//! image each robot's top-level package uses, with lines like `cs,flag_usa`. So an author can use a
//! country's flag or their own logo, and only the list decides which. Names match whatever their case.
//!
//! The limits are checked again here so that a bad file can never reach a page.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// The largest image file.
pub const MAX_BYTES: usize = 32 * 1024;
/// The largest width and height of an image.
pub const MAX_SIDE: u32 = 64;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// The images, and which one each package uses.
#[derive(Debug, Default)]
pub struct Logos {
    images: HashMap<String, Arc<Vec<u8>>>,
    packages: HashMap<String, String>,
}

impl Logos {
    /// Reads the images in a directory and the list of who uses which. A missing directory or list
    /// just means none.
    pub fn load(directory: &Path, list: Option<&Path>) -> Logos {
        let text = list.and_then(|list| std::fs::read_to_string(list).ok()).unwrap_or_default();
        let logos = Logos { images: load_dir(directory), packages: parse_list(&text) };
        for (package, image) in &logos.packages {
            if !logos.images.contains_key(image) {
                eprintln!("logos: {package} uses {image}, which has no valid file");
            }
        }
        logos
    }

    /// The bytes of an image, by its name.
    pub fn image(&self, name: &str) -> Option<&Arc<Vec<u8>>> {
        self.images.get(&key(name))
    }

    /// The name of the image a robot shows, if it has one.
    pub fn of_robot(&self, robot: &str) -> Option<&str> {
        let image = self.packages.get(&key(package_of(robot)))?;
        self.images.contains_key(image).then_some(image.as_str())
    }
}

/// The form of a package or image name that everything is stored and looked up by.
fn key(name: &str) -> String {
    name.to_ascii_lowercase()
}

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

/// Whether a text can be a package or an image name, which also keeps image names safe in paths.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Reads every valid image in a directory.
fn load_dir(directory: &Path) -> HashMap<String, Arc<Vec<u8>>> {
    let mut images = HashMap::new();
    let Ok(entries) = std::fs::read_dir(directory) else { return images };
    for entry in entries.flatten() {
        let path = entry.path();
        let file = path.file_name().and_then(|file| file.to_str()).unwrap_or_default();
        let Some(name) = file.strip_suffix(".png") else { continue };
        if !is_plain_name(name) {
            eprintln!(
                "logos: ignoring {file}: the name has characters other than letters, digits and _"
            );
            continue;
        }
        let checked = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| {
            check_png(&bytes)?;
            Ok(bytes)
        });
        match checked {
            Ok(bytes) => {
                images.insert(key(name), Arc::new(bytes));
            }
            Err(reason) => eprintln!("logos: ignoring {file}: {reason}"),
        }
    }
    images
}

/// Reads the `package,image` lines of the list.
fn parse_list(text: &str) -> HashMap<String, String> {
    let mut packages = HashMap::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        match line.split_once(',') {
            Some((package, image)) if is_plain_name(package) && is_plain_name(image) => {
                packages.insert(key(package), key(image));
            }
            _ => eprintln!("logos: ignoring a line in the list: {line}"),
        }
    }
    packages
}

/// The package whose image a robot uses: `claude.rm.Reckoner 0.1` uses `claude`.
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
    fn loads_only_valid_images() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("good.png"), png(16, 16)).unwrap();
        std::fs::write(dir.path().join("big.png"), png(100, 100)).unwrap();
        std::fs::write(dir.path().join("not_a_logo.txt"), "text").unwrap();
        std::fs::write(dir.path().join("bad-name.png"), png(16, 16)).unwrap();
        let images = load_dir(dir.path());
        assert_eq!(images.keys().collect::<Vec<_>>(), ["good"]);
    }

    #[test]
    fn missing_files_mean_no_images() {
        let logos = Logos::load(Path::new("/no/such/directory"), Some(Path::new("/no/such/list")));
        assert_eq!(logos.of_robot("a.One 1"), None);
    }

    #[test]
    fn the_list_decides_which_image_a_package_uses() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("flag_usa.png"), png(30, 18)).unwrap();
        std::fs::write(dir.path().join("own.png"), png(16, 16)).unwrap();
        let list = dir.path().join("list.csv");
        std::fs::write(&list, "cs,flag_usa\nclaude,own\nghost,flag_none\n").unwrap();
        let logos = Logos::load(dir.path(), Some(&list));

        assert_eq!(logos.of_robot("cs.Nene 1.1"), Some("flag_usa"));
        assert_eq!(logos.of_robot("claude.rm.Reckoner 0.1"), Some("own"));
        assert_eq!(logos.of_robot("ghost.One 1"), None, "listed, but the image does not exist");
        assert_eq!(logos.of_robot("own.One 1"), None, "an image name is not a package");
        assert!(logos.image("own").is_some());
    }

    #[test]
    fn names_match_whatever_their_case() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Flag_BRA.png"), png(30, 18)).unwrap();
        std::fs::write(dir.path().join("logo_wiki.png"), png(30, 18)).unwrap();
        let list = dir.path().join("list.csv");
        std::fs::write(&list, "axeBots,flag_bra\nsupersample,LOGO_Wiki\n").unwrap();
        let logos = Logos::load(dir.path(), Some(&list));

        assert_eq!(logos.of_robot("axeBots.Okami 1.04"), Some("flag_bra"));
        assert_eq!(logos.of_robot("axebots.Okami 1.04"), Some("flag_bra"));
        assert_eq!(logos.of_robot("SuperSample.SuperCrazy 1.0"), Some("logo_wiki"));
        assert!(logos.image("FLAG_bra").is_some());
    }

    #[test]
    fn bad_lines_in_the_list_are_ignored() {
        let packages =
            parse_list("cs,flag_usa\n\nnocomma\n../x,y\na,../y\na,b,c\n  ok,flag_jpn  \n");
        assert_eq!(packages.len(), 2);
        assert_eq!(packages["cs"], "flag_usa");
        assert_eq!(packages["ok"], "flag_jpn");
    }

    #[test]
    fn the_package_is_the_first_part_of_the_name() {
        assert_eq!(package_of("claude.rm.Reckoner 0.1"), "claude");
        assert_eq!(package_of("nopackage 1"), "nopackage");
    }
}
