#[cfg(debug_assertions)]
mod development;
#[cfg(all(not(debug_assertions), target_os = "macos"))]
mod macos;
#[cfg(not(debug_assertions))]
mod system;

#[cfg(debug_assertions)]
pub use development::{delete, read, save};

use std::fs::{File, OpenOptions};
use std::path::Path;

use crate::Error;

pub(crate) fn lock(path: &Path) -> Result<File, Error> {
    let parent = path.parent().ok_or(Error::InvalidStore)?;
    std::fs::create_dir_all(parent)?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let file = options.open(path)?;
    file.lock()?;
    Ok(file)
}
#[cfg(not(debug_assertions))]
pub use system::{delete, read, save};
