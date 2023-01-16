use std::{error::Error, fmt::Display, io};

use serde::{de::DeserializeOwned, Serialize};

#[derive(Debug)]
pub enum XdgError {
    IOError(io::Error),
    SerdeError(serde_json::Error),
    XdgError(xdg::BaseDirectoriesError),
}

impl From<io::Error> for XdgError {
    fn from(value: io::Error) -> Self {
        Self::IOError(value)
    }
}

impl From<serde_json::Error> for XdgError {
    fn from(value: serde_json::Error) -> Self {
        Self::SerdeError(value)
    }
}

impl From<xdg::BaseDirectoriesError> for XdgError {
    fn from(value: xdg::BaseDirectoriesError) -> Self {
        Self::XdgError(value)
    }
}

impl Display for XdgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XdgError::IOError(e) => write!(f, "xdg: {}", e),
            XdgError::SerdeError(e) => write!(f, "xdg: {}", e),
            XdgError::XdgError(e) => write!(f, "xdg: {}", e),
        }
    }
}

impl Error for XdgError {}

pub fn get_or_create_config<T>(filename: &str) -> Result<T, XdgError>
where
    T: Serialize + DeserializeOwned + Default,
{
    let path = xdg::BaseDirectories::with_prefix(crate::APP_NAME)?.place_config_file(filename)?;

    if path.exists() {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    } else {
        let default = T::default();
        std::fs::write(path, serde_json::to_string_pretty(&default)?)?;
        Ok(default)
    }
}

pub fn load_cache_if_exists<T>(filename: &str) -> Result<Option<T>, XdgError>
where
    T: DeserializeOwned,
{
    let path = xdg::BaseDirectories::with_prefix(crate::APP_NAME)?.place_state_file(filename)?;
    if path.exists() {
        Ok(Some(serde_json::from_str(&std::fs::read_to_string(path)?)?))
    } else {
        Ok(None)
    }
}

pub fn write_cache<T>(filename: &str, value: &T) -> Result<(), XdgError>
where
    T: Serialize,
{
    let path = xdg::BaseDirectories::with_prefix(crate::APP_NAME)?.place_state_file(filename)?;
    std::fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

pub fn delete_cache(filename: &str) -> Result<(), XdgError> {
    let path = xdg::BaseDirectories::with_prefix(crate::APP_NAME)?.place_state_file(filename)?;
    std::fs::remove_file(path)?;
    Ok(())
}
