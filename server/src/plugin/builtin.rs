use crate::Result;
use std::{fs, path::Path};

pub fn install(installed_root: &Path) -> Result<()> {
    fs::create_dir_all(installed_root)?;
    Ok(())
}
