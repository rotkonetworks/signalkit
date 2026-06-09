use std::path::PathBuf;

use crate::{Error, Result};

pub fn default_signal_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or(Error::NoHome)?;
    let flatpak = home.join(".var/app/org.signal.Signal/config/Signal");
    if flatpak.exists() {
        return Ok(flatpak);
    }
    Ok(home.join(".config/Signal"))
}
