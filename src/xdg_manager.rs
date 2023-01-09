use std::io;

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

pub fn load_cache_if_exists<T>(filename: &str) -> Result<T, XdgError>
where
    T: DeserializeOwned + Default,
{
    let path = xdg::BaseDirectories::with_prefix(crate::APP_NAME)?.place_state_file(filename)?;
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

pub fn write_cache_if_exists<T>(filename: &str, value: &T) -> Result<(), XdgError>
where
    T: Serialize + Default,
{
    let path = xdg::BaseDirectories::with_prefix(crate::APP_NAME)?.place_state_file(filename)?;
    std::fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}
