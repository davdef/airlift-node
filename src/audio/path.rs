use anyhow::{bail, Result};
use std::path::{Component, Path, PathBuf};

pub fn sanitize_audio_path(path: &str) -> Result<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        bail!("path must not be empty");
    }
    if trimmed.contains('\0') {
        bail!("path must not contain null bytes");
    }

    let mut sanitized = PathBuf::new();
    let mut has_component = false;

    for component in Path::new(trimmed).components() {
        match component {
            Component::Prefix(prefix) => {
                sanitized.push(prefix.as_os_str());
                has_component = true;
            }
            Component::RootDir => {
                sanitized.push(component.as_os_str());
                has_component = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                bail!("path must not contain '..' segments");
            }
            Component::Normal(part) => {
                sanitized.push(part);
                has_component = true;
            }
        }
    }

    if !has_component {
        bail!("path must contain at least one component");
    }

    Ok(sanitized)
}
